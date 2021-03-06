use crate::{
    create_discrete_increment_interval, create_unit_value_interval, full_unit_interval,
    negative_if, AbsoluteValue, ButtonUsage, ControlType, ControlValue, DiscreteIncrement,
    DiscreteValue, EncoderUsage, Fraction, Interval, MinIsMaxBehavior, OutOfRangeBehavior,
    PressDurationProcessor, TakeoverMode, Target, Transformation, UnitIncrement, UnitValue,
    BASE_EPSILON,
};
use derive_more::Display;
use enum_iterator::IntoEnumIterator;
use num_enum::{IntoPrimitive, TryFromPrimitive};
#[cfg(feature = "serde_repr")]
use serde_repr::{Deserialize_repr, Serialize_repr};

/// When interpreting target value, make only 4 fractional digits matter.
///
/// If we don't do this and target min == target max, even the slightest imprecision of the actual
/// target value (which in practice often occurs with FX parameters not taking exactly the desired
/// value) could result in a totally different feedback value. Maybe it would be better to determine
/// the epsilon dependent on the source precision (e.g. 1.0/128.0 in case of short MIDI messages)
/// but right now this should suffice to solve the immediate problem.  
pub const FEEDBACK_EPSILON: f64 = BASE_EPSILON;

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct ModeControlOptions {
    pub enforce_rotate: bool,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct ModeFeedbackOptions {
    pub source_is_virtual: bool,
    pub max_discrete_source_value: Option<u32>,
}

/// Settings for processing all kinds of control values.
///
/// ## How relative control values are processed (or button taps interpreted as increments).
///
/// Here's an overview in which cases step counts are used and in which step sizes.
/// This is the same, no matter if the source emits relative increments or absolute values
/// ("relative one-direction mode").
///   
/// - Target wants relative increments: __Step counts__
///     - Example: Action with invocation type "Relative"
///     - Displayed as: "{count} x"
/// - Target wants absolute values
///     - Target is continuous, optionally roundable: __Step sizes__
///         - Example: Track volume
///         - Displayed as: "{size} {unit}"
///     - Target is discrete: __Step counts__
///         - Example: FX preset, some FX params
///         - Displayed as: "{count} x" or "{count}" (former if source emits increments) TODO I
///           think now we have only the "x" variant
#[derive(Clone, Debug)]
pub struct Mode<T: Transformation> {
    pub absolute_mode: AbsoluteMode,
    pub source_value_interval: Interval<UnitValue>,
    pub discrete_source_value_interval: Interval<u32>,
    pub target_value_interval: Interval<UnitValue>,
    pub discrete_target_value_interval: Interval<u32>,
    /// Negative increments represent fractions (throttling), e.g. -2 fires an increment every
    /// 2nd time only.
    pub step_count_interval: Interval<DiscreteIncrement>,
    pub step_size_interval: Interval<UnitValue>,
    pub jump_interval: Interval<UnitValue>,
    pub discrete_jump_interval: Interval<u32>,
    // TODO-low Not cool to make this public. Maybe derive a builder for this beast.
    pub press_duration_processor: PressDurationProcessor,
    pub takeover_mode: TakeoverMode,
    pub encoder_usage: EncoderUsage,
    pub button_usage: ButtonUsage,
    pub reverse: bool,
    pub rotate: bool,
    pub round_target_value: bool,
    pub out_of_range_behavior: OutOfRangeBehavior,
    pub control_transformation: Option<T>,
    pub feedback_transformation: Option<T>,
    pub convert_relative_to_absolute: bool,
    /// For relative-to-absolute mode
    pub current_absolute_value: UnitValue,
    pub discrete_current_absolute_value: u32,
    /// Counter for implementing throttling.
    ///
    /// Throttling is implemented by spitting out control values only every nth time. The counter
    /// can take positive or negative values in order to detect direction changes. This is positive
    /// when the last change was a positive increment and negative when the last change was a
    /// negative increment.
    pub increment_counter: i32,
    /// For absolute-to-relative mode and value-scaling takeover mode.
    pub previous_absolute_control_value: Option<UnitValue>,
    pub discrete_previous_absolute_control_value: Option<u32>,
    pub use_discrete_processing: bool,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, IntoEnumIterator, TryFromPrimitive, IntoPrimitive, Display,
)]
#[cfg_attr(feature = "serde_repr", derive(Serialize_repr, Deserialize_repr))]
#[repr(usize)]
pub enum AbsoluteMode {
    #[display(fmt = "Normal")]
    Normal = 0,
    #[display(fmt = "Incremental buttons")]
    IncrementalButtons = 1,
    #[display(fmt = "Toggle buttons")]
    ToggleButtons = 2,
}

impl Default for AbsoluteMode {
    fn default() -> Self {
        AbsoluteMode::Normal
    }
}

impl<T: Transformation> Default for Mode<T> {
    fn default() -> Self {
        Mode {
            absolute_mode: AbsoluteMode::Normal,
            source_value_interval: full_unit_interval(),
            discrete_source_value_interval: full_discrete_interval(),
            target_value_interval: full_unit_interval(),
            discrete_target_value_interval: full_discrete_interval(),
            step_size_interval: default_step_size_interval(),
            step_count_interval: default_step_count_interval(),
            jump_interval: full_unit_interval(),
            discrete_jump_interval: full_discrete_interval(),
            press_duration_processor: Default::default(),
            takeover_mode: Default::default(),
            button_usage: Default::default(),
            encoder_usage: Default::default(),
            reverse: false,
            round_target_value: false,
            out_of_range_behavior: OutOfRangeBehavior::MinOrMax,
            control_transformation: None,
            feedback_transformation: None,
            rotate: false,
            increment_counter: 0,
            convert_relative_to_absolute: false,
            current_absolute_value: UnitValue::MIN,
            discrete_current_absolute_value: 0,
            previous_absolute_control_value: None,
            discrete_previous_absolute_control_value: None,
            use_discrete_processing: false,
        }
    }
}

impl<T: Transformation> Mode<T> {
    /// Processes the given control value and maybe returns an appropriate target control value.
    ///
    /// `None` either means ignored or target value already has desired value.
    #[cfg(test)]
    fn control<'a, C: Copy>(
        &mut self,
        control_value: ControlValue,
        target: &impl Target<'a, Context = C>,
        context: C,
    ) -> Option<ControlValue> {
        self.control_with_options(
            control_value,
            target,
            context,
            ModeControlOptions::default(),
        )?
        .into()
    }

    /// Processes the given control value and maybe returns an appropriate target control value.
    ///
    /// `None` means the incoming source control value doesn't reach the target because it's
    /// filtered out (e.g. because of button filter "Press only").
    pub fn control_with_options<'a, C: Copy>(
        &mut self,
        control_value: ControlValue,
        target: &impl Target<'a, Context = C>,
        context: C,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<ControlValue>> {
        match control_value {
            ControlValue::Relative(i) => self.control_relative(i, target, context, options),
            ControlValue::AbsoluteContinuous(v) => {
                self.control_absolute(AbsoluteValue::Continuous(v), target, context, true, options)
            }
            ControlValue::AbsoluteDiscrete(v) => {
                self.control_absolute(AbsoluteValue::Discrete(v), target, context, true, options)
            }
        }
    }

    #[cfg(test)]
    fn feedback(&self, target_value: AbsoluteValue) -> Option<AbsoluteValue> {
        self.feedback_with_options(target_value, ModeFeedbackOptions::default())
    }

    /// Takes a target value, interprets and transforms it conforming to mode rules and
    /// maybe returns an appropriate source value that should be sent to the source.
    pub fn feedback_with_options(
        &self,
        target_value: AbsoluteValue,
        options: ModeFeedbackOptions,
    ) -> Option<AbsoluteValue> {
        let v = target_value;
        // 4. Filter and Apply target interval (normalize)
        let interval_match_result = v.matches_tolerant(
            &self.target_value_interval,
            &self.discrete_target_value_interval,
            FEEDBACK_EPSILON,
        );
        let (mut v, min_is_max_behavior) = if interval_match_result.matches() {
            // Target value is within target value interval
            (v, MinIsMaxBehavior::PreferOne)
        } else {
            // Target value is outside target value interval
            self.out_of_range_behavior.process(
                v,
                interval_match_result,
                &self.target_value_interval,
                &self.discrete_target_value_interval,
            )?
        };
        // Tolerant interval bounds test because of https://github.com/helgoboss/realearn/issues/263.
        // TODO-medium The most elaborate solution to deal with discrete values would be to actually
        //  know which interval of floating point values represents a specific discrete target value.
        //  However, is there a generic way to know that? Taking the target step size as epsilon in this
        //  case sounds good but we still don't know if the target respects approximate values, if it
        //  rounds them or uses more a ceil/floor approach ... I don't think this is standardized for
        //  VST parameters. We could solve it for our own parameters in future. Until then, having a
        //  fixed epsilon deals at least with most issues I guess.
        v = v.normalize(
            &self.target_value_interval,
            &self.discrete_target_value_interval,
            min_is_max_behavior,
            self.use_discrete_processing,
            FEEDBACK_EPSILON,
        );
        // 3. Apply reverse
        if self.reverse {
            let normalized_max_discrete_source_value = options
                .max_discrete_source_value
                .map(|m| self.discrete_source_value_interval.normalize_to_min(m));
            v = v.inverse(normalized_max_discrete_source_value);
        };
        // 2. Apply transformation
        if let Some(transformation) = self.feedback_transformation.as_ref() {
            if let Ok(res) = v.transform(transformation, Some(v), self.use_discrete_processing) {
                v = res;
            }
        };
        // 1. Apply source interval
        v = v.denormalize(
            &self.source_value_interval,
            &self.discrete_source_value_interval,
            self.use_discrete_processing,
            options.max_discrete_source_value,
        );
        // Result
        if !self.use_discrete_processing && !options.source_is_virtual {
            // If discrete processing is not explicitly enabled, we must NOT send discrete values to
            // a real (non-virtual) source! This is not just for backward compatibility. It would change
            // how discrete sources react in a surprising way (discrete behavior without having
            // discrete processing enabled).
            v = v.to_continuous_value();
        };
        Some(v)
    }

    /// If this returns `true`, the `poll` method should be called, on a regular basis.
    pub fn wants_to_be_polled(&self) -> bool {
        self.press_duration_processor.wants_to_be_polled()
    }

    /// This function should be called regularly if the features are needed that are driven by a
    /// timer (fire on length min, turbo, etc.). Returns a target control value whenever it's time
    /// to fire.
    pub fn poll<'a, C: Copy>(
        &mut self,
        target: &impl Target<'a, Context = C>,
        context: C,
    ) -> Option<ModeControlResult<ControlValue>> {
        let control_value = self.press_duration_processor.poll()?;
        self.control_absolute(
            control_value,
            target,
            context,
            false,
            ModeControlOptions::default(),
        )
    }

    fn control_relative<'a, C: Copy>(
        &mut self,
        i: DiscreteIncrement,
        target: &impl Target<'a, Context = C>,
        context: C,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<ControlValue>> {
        match self.encoder_usage {
            EncoderUsage::IncrementOnly if !i.is_positive() => return None,
            EncoderUsage::DecrementOnly if i.is_positive() => return None,
            _ => {}
        };
        if self.convert_relative_to_absolute {
            Some(
                self.control_relative_to_absolute(i, target, context, options)?
                    .map(|v| ControlValue::AbsoluteContinuous(v.to_unit_value())),
            )
        } else {
            self.control_relative_normal(i, target, context, options)
        }
    }

    fn control_absolute<'a, C: Copy>(
        &mut self,
        v: AbsoluteValue,
        target: &impl Target<'a, Context = C>,
        context: C,
        consider_press_duration: bool,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<ControlValue>> {
        let v = if consider_press_duration {
            self.press_duration_processor.process_press_or_release(v)?
        } else {
            v
        };
        use AbsoluteMode::*;
        match self.absolute_mode {
            Normal => Some(
                self.control_absolute_normal(v, target, context)?
                    .map(ControlValue::from_absolute),
            ),
            IncrementalButtons => self.control_absolute_incremental_buttons(
                v.to_unit_value(),
                target,
                context,
                options,
            ),
            ToggleButtons => Some(
                self.control_absolute_toggle_buttons(v, target, context)?
                    .map(|v| ControlValue::AbsoluteContinuous(v.to_unit_value())),
            ),
        }
    }

    /// Processes the given control value in absolute mode and maybe returns an appropriate target
    /// value.
    fn control_absolute_normal<'a, C: Copy>(
        &mut self,
        control_value: AbsoluteValue,
        target: &impl Target<'a, Context = C>,
        context: C,
    ) -> Option<ModeControlResult<AbsoluteValue>> {
        // Memorize as previous value for next control cycle.
        let previous_control_value = self
            .previous_absolute_control_value
            .replace(control_value.to_unit_value());
        // Filter
        match self.button_usage {
            ButtonUsage::PressOnly if control_value.is_zero() => return None,
            ButtonUsage::ReleaseOnly if !control_value.is_zero() => return None,
            _ => {}
        };
        let interval_match_result = control_value.matches_tolerant(
            &self.source_value_interval,
            &self.discrete_source_value_interval,
            BASE_EPSILON,
        );
        let (source_bound_value, min_is_max_behavior) = if interval_match_result.matches() {
            // Control value is within source value interval
            (control_value, MinIsMaxBehavior::PreferOne)
        } else {
            // Control value is outside source value interval
            self.out_of_range_behavior.process(
                control_value,
                interval_match_result,
                &self.source_value_interval,
                &self.discrete_source_value_interval,
            )?
        };
        // Control value is within source value interval
        let current_target_value = target.current_value(context);
        let control_type = target.control_type();
        let pepped_up_control_value = self.pep_up_control_value(
            source_bound_value,
            control_type,
            current_target_value,
            min_is_max_behavior,
        );
        self.hitting_target_considering_max_jump(
            pepped_up_control_value,
            current_target_value,
            control_type,
            previous_control_value.map(AbsoluteValue::Continuous),
        )
    }

    /// "Incremental buttons" mode (convert absolute button presses to relative increments)
    fn control_absolute_incremental_buttons<'a, C: Copy>(
        &mut self,
        control_value: UnitValue,
        target: &impl Target<'a, Context = C>,
        context: C,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<ControlValue>> {
        // TODO-high-discrete In discrete processing, don't interpret current target value as percentage!
        if control_value.is_zero()
            || !self
                .source_value_interval
                .value_matches_tolerant(control_value, BASE_EPSILON)
                .matches()
        {
            return None;
        }
        use ControlType::*;
        let control_type = target.control_type();
        match control_type {
            AbsoluteContinuous
            | AbsoluteContinuousRoundable { .. }
            // TODO-low I think trigger and switch targets don't make sense at all here because
            //  instead of +/- n they need just "trigger!" or "on/off!". 
            | AbsoluteContinuousRetriggerable => {
                // Continuous target
                //
                // Settings:
                // - Source value interval (for setting the input interval of relevant source
                //   values)
                // - Minimum target step size (enables accurate minimum increment, atomic)
                // - Maximum target step size (enables accurate maximum increment, clamped)
                // - Target value interval (absolute, important for rotation only, clamped)
                let step_size_value = control_value
                    .normalize(
                        &self.source_value_interval,
                        MinIsMaxBehavior::PreferOne,
                        BASE_EPSILON
                    )
                    .denormalize(&self.step_size_interval);
                let step_size_increment =
                    step_size_value.to_increment(negative_if(self.reverse))?;
                self.hit_target_absolutely_with_unit_increment(
                    step_size_increment,
                    self.step_size_interval.min_val(),
                    target.current_value(context)?.to_unit_value(),
                    options,
                    control_type
                )
            }
            AbsoluteDiscrete { atomic_step_size } => {
                // Discrete target
                //
                // Settings:
                // - Source value interval (for setting the input interval of relevant source
                //   values)
                // - Minimum target step count (enables accurate normal/minimum increment, atomic)
                // - Target value interval (absolute, important for rotation only, clamped)
                // - Maximum target step count (enables accurate maximum increment, clamped)
                let discrete_increment = self.convert_to_discrete_increment(control_value)?;
                self.hit_discrete_target_absolutely(discrete_increment, atomic_step_size, options, control_type, || {
                    target.current_value(context)
                })
            }
            Relative
            // This is cool! With this, we can make controllers without encoders simulate them
            // by assigning one - button and one + button to the same virtual multi target.
            // Of course, all we can deliver is increments/decrements since virtual targets 
            // don't provide a current target value. But we also don't need it because all we
            // want to do is simulate an encoder.
            | VirtualMulti => {
                // Target wants increments so we just generate them e.g. depending on how hard the
                // button has been pressed
                //
                // - Source value interval (for setting the input interval of relevant source
                //   values)
                // - Minimum target step count (enables accurate normal/minimum increment, atomic)
                // - Maximum target step count (enables accurate maximum increment, mapped)
                let discrete_increment = self.convert_to_discrete_increment(control_value)?;
                Some(ModeControlResult::HitTarget(ControlValue::Relative(discrete_increment)))
            }
            VirtualButton => {
                // This doesn't make sense at all. Buttons just need to be triggered, not fed with
                // +/- n.
                None
            }
        }
    }

    fn control_absolute_toggle_buttons<'a, C: Copy>(
        &mut self,
        control_value: AbsoluteValue,
        target: &impl Target<'a, Context = C>,
        context: C,
    ) -> Option<ModeControlResult<AbsoluteValue>> {
        // TODO-high-discrete In discrete processing, don't interpret current target value as
        //  percentage!
        if control_value.is_zero() {
            return None;
        }
        let center_target_value = self.target_value_interval.center();
        // Nothing we can do if we can't get the current target value. This shouldn't happen
        // usually because virtual targets are not supposed to be used with toggle mode.
        let current_target_value = target.current_value(context)?;
        let desired_target_value = if current_target_value.to_unit_value() > center_target_value {
            self.target_value_interval.min_val()
        } else {
            self.target_value_interval.max_val()
        };
        // If the settings make sense for toggling, the desired target value should *always*
        // be different than the current value. Therefore no need to check if the target value
        // already has that value.
        let final_absolute_value = self.get_final_absolute_value(
            AbsoluteValue::Continuous(desired_target_value),
            target.control_type(),
        );
        Some(ModeControlResult::HitTarget(final_absolute_value))
    }

    // Relative-to-absolute conversion mode.
    fn control_relative_to_absolute<'a, C: Copy>(
        &mut self,
        discrete_increment: DiscreteIncrement,
        target: &impl Target<'a, Context = C>,
        context: C,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<AbsoluteValue>> {
        // Convert to absolute value
        let mut inc = discrete_increment.to_unit_increment(self.step_size_interval.min_val())?;
        inc = inc.clamp_to_interval(&self.step_size_interval)?;
        let full_unit_interval = full_unit_interval();
        let abs_input_value = if options.enforce_rotate || self.rotate {
            self.current_absolute_value
                .add_rotating(inc, &full_unit_interval, BASE_EPSILON)
        } else {
            self.current_absolute_value
                .add_clamping(inc, &full_unit_interval, BASE_EPSILON)
        };
        self.current_absolute_value = abs_input_value;
        // Do the usual absolute processing
        self.control_absolute_normal(AbsoluteValue::Continuous(abs_input_value), target, context)
    }

    // Classic relative mode: We are getting encoder increments from the source.
    // We don't need source min/max config in this case. At least I can't think of a use case
    // where one would like to totally ignore especially slow or especially fast encoder movements,
    // I guess that possibility would rather cause irritation.
    fn control_relative_normal<'a, C: Copy>(
        &mut self,
        discrete_increment: DiscreteIncrement,
        target: &impl Target<'a, Context = C>,
        context: C,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<ControlValue>> {
        use ControlType::*;
        let control_type = target.control_type();
        match control_type {
            AbsoluteContinuous
            | AbsoluteContinuousRoundable { .. }
            // TODO-low Controlling a switch/trigger target with +/- n doesn't make sense.
            | AbsoluteContinuousRetriggerable => {
                // Continuous target
                //
                // Settings which are always necessary:
                // - Minimum target step size (enables accurate minimum increment, atomic)
                // - Target value interval (absolute, important for rotation only, clamped)
                //
                // Settings which are necessary in order to support >1-increments:
                // - Maximum target step size (enables accurate maximum increment, clamped)
                let potentially_reversed_increment = if self.reverse {
                    discrete_increment.inverse()
                } else {
                    discrete_increment
                };
                let unit_increment = potentially_reversed_increment
                    .to_unit_increment(self.step_size_interval.min_val())?;
                let clamped_unit_increment =
                    unit_increment.clamp_to_interval(&self.step_size_interval)?;
                self.hit_target_absolutely_with_unit_increment(
                    clamped_unit_increment,
                    self.step_size_interval.min_val(),
                    target.current_value(context)?.to_unit_value(),
                    options,
                    control_type
                )
            }
            AbsoluteDiscrete { atomic_step_size } => {
                // Discrete target
                //
                // Settings which are always necessary:
                // - Minimum target step count (enables accurate normal/minimum increment, atomic)
                // - Target value interval (absolute, important for rotation only, clamped)
                //
                // Settings which are necessary in order to support >1-increments:
                // - Maximum target step count (enables accurate maximum increment, clamped)
                let pepped_up_increment = self.pep_up_discrete_increment(discrete_increment)?;
                self.hit_discrete_target_absolutely(pepped_up_increment, atomic_step_size, options, control_type, || {
                    target.current_value(context)
                })
            }
            Relative | VirtualMulti => {
                // Target wants increments so we just forward them after some preprocessing
                //
                // Settings which are always necessary:
                // - Minimum target step count (enables accurate normal/minimum increment, clamped)
                //
                // Settings which are necessary in order to support >1-increments:
                // - Maximum target step count (enables accurate maximum increment, clamped)
                let pepped_up_increment = self.pep_up_discrete_increment(discrete_increment)?;
                Some(ModeControlResult::HitTarget(ControlValue::Relative(pepped_up_increment)))
            }
            VirtualButton => {
                // Controlling a button target with +/- n doesn't make sense.
                None
            }
        }
    }

    fn pep_up_control_value(
        &self,
        control_value: AbsoluteValue,
        control_type: ControlType,
        current_target_value: Option<AbsoluteValue>,
        min_is_max_behavior: MinIsMaxBehavior,
    ) -> AbsoluteValue {
        // 1. Apply source interval
        let mut v = control_value.normalize(
            &self.source_value_interval,
            &self.discrete_source_value_interval,
            min_is_max_behavior,
            self.use_discrete_processing,
            BASE_EPSILON,
        );
        // 2. Apply transformation
        if let Some(transformation) = self.control_transformation.as_ref() {
            if let Ok(res) = v.transform(
                transformation,
                current_target_value,
                self.use_discrete_processing,
            ) {
                v = res;
            }
        };
        // 3. Apply reverse
        if self.reverse {
            // We must normalize the target value value and use it in the inversion operation.
            // As an alternative, we could BEFORE doing all that stuff homogenize the source and
            // target intervals to have the same (minimum) size?
            let normalized_max_discrete_target_value = control_type
                .discrete_max()
                .map(|m| self.discrete_target_value_interval.normalize_to_min(m));
            // If this is a discrete target (which reports a discrete maximum) and discrete
            // processing is disabled, the reverse operation must use a "scaling reverse", not a
            // "subtraction reverse". Therefore we must turn a discrete control value into a
            // continuous value in this case before applying the reverse operation.
            if normalized_max_discrete_target_value.is_some() && !self.use_discrete_processing {
                v = v.to_continuous_value();
            }
            v = v.inverse(normalized_max_discrete_target_value);
        };
        // 4. Apply target interval
        v = v.denormalize(
            &self.target_value_interval,
            &self.discrete_target_value_interval,
            self.use_discrete_processing,
            control_type.discrete_max(),
        );
        // 5. Apply rounding
        if self.round_target_value {
            v = v.round(control_type);
        };
        v
    }

    fn hitting_target_considering_max_jump(
        &mut self,
        control_value: AbsoluteValue,
        current_target_value: Option<AbsoluteValue>,
        control_type: ControlType,
        previous_control_value: Option<AbsoluteValue>,
    ) -> Option<ModeControlResult<AbsoluteValue>> {
        let current_target_value = match current_target_value {
            // No target value available ... just deliver! Virtual targets take this shortcut.
            None => {
                return Some(ModeControlResult::HitTarget(
                    self.get_final_absolute_value(control_value, control_type),
                ))
            }
            Some(v) => v,
        };
        if (!self.use_discrete_processing || control_value.is_continuous())
            && self.jump_interval.is_full()
        {
            // No jump restrictions whatsoever
            return self.hit_if_changed(control_value, current_target_value, control_type);
        }
        let distance = control_value.calc_distance_from(current_target_value);
        if distance.is_greater_than(
            self.jump_interval.max_val(),
            self.discrete_jump_interval.max_val(),
        ) {
            // Distance is too large
            use TakeoverMode::*;
            return match self.takeover_mode {
                Pickup => {
                    // Scaling not desired. Do nothing.
                    None
                }
                Parallel => {
                    // TODO-high-discrete Implement advanced takeover modes for discrete values, too
                    // TODO-medium Add tests for advanced takeover modes!
                    if let Some(prev) = previous_control_value {
                        let relative_increment =
                            control_value.to_unit_value() - prev.to_unit_value();
                        if relative_increment == 0.0 {
                            None
                        } else {
                            let relative_increment = UnitIncrement::new(relative_increment);
                            let restrained_increment =
                                relative_increment.clamp_to_interval(&self.jump_interval)?;
                            let final_target_value =
                                current_target_value.to_unit_value().add_clamping(
                                    restrained_increment,
                                    &self.target_value_interval,
                                    BASE_EPSILON,
                                );
                            self.hit_if_changed(
                                AbsoluteValue::Continuous(final_target_value),
                                current_target_value,
                                control_type,
                            )
                        }
                    } else {
                        // We can't know the direction if we don't have a previous value.
                        // Wait for next incoming value.
                        None
                    }
                }
                LongTimeNoSee => {
                    let approach_distance = distance.denormalize(
                        &self.jump_interval,
                        &self.discrete_jump_interval,
                        self.use_discrete_processing,
                        control_type.discrete_max(),
                    );
                    let approach_increment =
                        approach_distance.to_unit_value().to_increment(negative_if(
                            control_value.to_unit_value() < current_target_value.to_unit_value(),
                        ))?;
                    let final_target_value = current_target_value.to_unit_value().add_clamping(
                        approach_increment,
                        &self.target_value_interval,
                        BASE_EPSILON,
                    );
                    self.hit_if_changed(
                        AbsoluteValue::Continuous(final_target_value),
                        current_target_value,
                        control_type,
                    )
                }
                CatchUp => {
                    if let Some(prev) = previous_control_value {
                        let prev = prev.to_unit_value();
                        let relative_increment = control_value.to_unit_value() - prev;
                        if relative_increment == 0.0 {
                            None
                        } else {
                            let goes_up = relative_increment.is_sign_positive();
                            let source_distance_from_border = if goes_up {
                                1.0 - prev.get()
                            } else {
                                prev.get()
                            };
                            let current_target_value = current_target_value.to_unit_value();
                            let target_distance_from_border = if goes_up {
                                1.0 - current_target_value.get()
                            } else {
                                current_target_value.get()
                            };
                            if source_distance_from_border == 0.0
                                || target_distance_from_border == 0.0
                            {
                                None
                            } else {
                                let scaled_increment = relative_increment
                                    * target_distance_from_border
                                    / source_distance_from_border;
                                let scaled_increment = UnitIncrement::new(scaled_increment);
                                let restrained_increment =
                                    scaled_increment.clamp_to_interval(&self.jump_interval)?;
                                let final_target_value = current_target_value.add_clamping(
                                    restrained_increment,
                                    &self.target_value_interval,
                                    BASE_EPSILON,
                                );
                                self.hit_if_changed(
                                    AbsoluteValue::Continuous(final_target_value),
                                    AbsoluteValue::Continuous(current_target_value),
                                    control_type,
                                )
                            }
                        }
                    } else {
                        // We can't know the direction if we don't have a previous value.
                        // Wait for next incoming value.
                        None
                    }
                }
            };
        }
        // Distance is not too large
        if distance.is_lower_than(
            self.jump_interval.min_val(),
            self.discrete_jump_interval.min_val(),
        ) {
            return None;
        }
        // Distance is also not too small
        self.hit_if_changed(control_value, current_target_value, control_type)
    }

    fn hit_if_changed(
        &self,
        desired_target_value: AbsoluteValue,
        current_target_value: AbsoluteValue,
        control_type: ControlType,
    ) -> Option<ModeControlResult<AbsoluteValue>> {
        if !control_type.is_retriggerable()
            && current_target_value.has_same_effect_as(desired_target_value)
        {
            return Some(ModeControlResult::LeaveTargetUntouched(
                desired_target_value,
            ));
        }
        let final_value = self.get_final_absolute_value(desired_target_value, control_type);
        Some(ModeControlResult::HitTarget(final_value))
    }

    fn get_final_absolute_value(
        &self,
        desired_target_value: AbsoluteValue,
        control_type: ControlType,
    ) -> AbsoluteValue {
        if self.use_discrete_processing || control_type.is_virtual() {
            desired_target_value
        } else {
            // If discrete processing is not explicitly enabled, we must NOT send discrete values to
            // a real target! This is not just for backward compatibility. It would change how
            // discrete targets react in a surprising way (discrete behavior without having discrete
            // processing enabled). The reason why we don't "go continuous" right at the start of
            // the processing in this case is that we also have mappings with virtual targets. It's
            // important that they don't destroy the discreteness of a value, otherwise existing
            // controller presets which don't have "discrete processing" enabled would not be
            // compatible with main mappings that have discrete targets and want to use discrete
            // processing. There would have been other ways to deal with this (e.g. migration) but
            // the concept of letting a discrete value survive as long as possible (= not turning
            // it into a continuous one and thereby losing information) sounds like a good idea in
            // general.
            AbsoluteValue::Continuous(desired_target_value.to_unit_value())
        }
    }

    fn hit_discrete_target_absolutely(
        &self,
        discrete_increment: DiscreteIncrement,
        target_step_size: UnitValue,
        options: ModeControlOptions,
        control_type: ControlType,
        current_value: impl Fn() -> Option<AbsoluteValue>,
    ) -> Option<ModeControlResult<ControlValue>> {
        if self.use_discrete_processing {
            // Discrete processing for discrete target. Good!
            match current_value()? {
                AbsoluteValue::Continuous(_) => {
                    // But target reports continuous value!? Shouldn't happen. Whatever, fall back
                    // to continuous processing.
                    self.hit_target_absolutely_with_unit_increment(
                        discrete_increment.to_unit_increment(target_step_size)?,
                        target_step_size,
                        current_value()?.to_unit_value(),
                        options,
                        control_type,
                    )
                }
                AbsoluteValue::Discrete(f) => self.hit_target_absolutely_with_discrete_increment(
                    discrete_increment,
                    f,
                    options,
                    control_type,
                ),
            }
        } else {
            // Continuous processing although target is discrete. Kept for backward compatibility.
            self.hit_target_absolutely_with_unit_increment(
                discrete_increment.to_unit_increment(target_step_size)?,
                target_step_size,
                current_value()?.to_unit_value(),
                options,
                control_type,
            )
        }
    }

    fn hit_target_absolutely_with_unit_increment(
        &self,
        increment: UnitIncrement,
        grid_interval_size: UnitValue,
        current_target_value: UnitValue,
        options: ModeControlOptions,
        control_type: ControlType,
    ) -> Option<ModeControlResult<ControlValue>> {
        let snapped_target_value_interval = Interval::new(
            self.target_value_interval
                .min_val()
                .snap_to_grid_by_interval_size(grid_interval_size),
            self.target_value_interval
                .max_val()
                .snap_to_grid_by_interval_size(grid_interval_size),
        );
        // The add functions don't add anything if the current target value is not within the target
        // interval in the first place. Instead they return one of the interval bounds. One issue
        // that might occur is that the current target value only *appears* out-of-range
        // because of numerical inaccuracies. That could lead to frustrating "it doesn't move"
        // experiences. Therefore we snap the current target value to grid first in that case.
        let mut v = if current_target_value.is_within_interval(&snapped_target_value_interval) {
            current_target_value
        } else {
            current_target_value.snap_to_grid_by_interval_size(grid_interval_size)
        };
        v = if options.enforce_rotate || self.rotate {
            v.add_rotating(increment, &snapped_target_value_interval, BASE_EPSILON)
        } else {
            v.add_clamping(increment, &snapped_target_value_interval, BASE_EPSILON)
        };
        if v == current_target_value {
            return Some(ModeControlResult::LeaveTargetUntouched(
                ControlValue::AbsoluteContinuous(v),
            ));
        }
        let final_absolute_value =
            self.get_final_absolute_value(AbsoluteValue::Continuous(v), control_type);
        Some(ModeControlResult::HitTarget(ControlValue::from_absolute(
            final_absolute_value,
        )))
    }

    fn hit_target_absolutely_with_discrete_increment(
        &self,
        increment: DiscreteIncrement,
        current_target_value: Fraction,
        options: ModeControlOptions,
        control_type: ControlType,
    ) -> Option<ModeControlResult<ControlValue>> {
        let mut v = current_target_value;
        v = if options.enforce_rotate || self.rotate {
            v.add_rotating(increment, &self.discrete_target_value_interval)
        } else {
            v.add_clamping(increment, &self.discrete_target_value_interval)
        };
        if let Some(target_max) = control_type.discrete_max() {
            v = v.with_max_clamped(target_max);
        }
        if v.actual() == current_target_value.actual() {
            return Some(ModeControlResult::LeaveTargetUntouched(
                ControlValue::AbsoluteDiscrete(v),
            ));
        }
        let final_absolute_value =
            self.get_final_absolute_value(AbsoluteValue::Discrete(v), control_type);
        Some(ModeControlResult::HitTarget(ControlValue::from_absolute(
            final_absolute_value,
        )))
    }

    fn pep_up_discrete_increment(
        &mut self,
        increment: DiscreteIncrement,
    ) -> Option<DiscreteIncrement> {
        let factor = increment.clamp_to_interval(&self.step_count_interval);
        let actual_increment = if factor.is_positive() {
            factor
        } else {
            let nth = factor.get().abs() as u32;
            let (fire, new_counter_value) = self.its_time_to_fire(nth, increment.signum());
            self.increment_counter = new_counter_value;
            if !fire {
                return None;
            }
            DiscreteIncrement::new(1)
        };
        let clamped_increment = actual_increment.with_direction(increment.signum());
        let result = if self.reverse {
            clamped_increment.inverse()
        } else {
            clamped_increment
        };
        Some(result)
    }

    /// `nth` stands for "fire every nth time". `direction_signum` is either +1 or -1.
    fn its_time_to_fire(&self, nth: u32, direction_signum: i32) -> (bool, i32) {
        if self.increment_counter == 0 {
            // Initial fire
            return (true, direction_signum);
        }
        let positive_increment_counter = self.increment_counter.abs() as u32;
        if positive_increment_counter >= nth {
            // After having waited for a few increments, fire again.
            return (true, direction_signum);
        }
        (false, self.increment_counter + direction_signum)
    }

    fn convert_to_discrete_increment(
        &mut self,
        control_value: UnitValue,
    ) -> Option<DiscreteIncrement> {
        let factor = control_value
            .normalize(
                &self.source_value_interval,
                MinIsMaxBehavior::PreferOne,
                BASE_EPSILON,
            )
            .denormalize_discrete_increment(&self.step_count_interval);
        // This mode supports positive increment only.
        let discrete_value = if factor.is_positive() {
            factor.to_value()
        } else {
            let nth = factor.get().abs() as u32;
            let (fire, new_counter_value) = self.its_time_to_fire(nth, 1);
            self.increment_counter = new_counter_value;
            if !fire {
                return None;
            }
            DiscreteValue::new(1)
        };
        discrete_value.to_increment(negative_if(self.reverse))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::mode::test_util::{TestTarget, TestTransformation};
    use crate::{create_unit_value_interval, ControlType, Fraction};
    use approx::*;

    mod absolute_normal {
        use super::*;

        mod continuous_processing {
            use super::*;

            #[test]
            fn default() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.3779527559055118)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(0, 127), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_eq!(mode.control(abs_con(0.3779527559055118), &target, ()), None);
                assert_eq!(mode.control(abs_dis(48, 127), &target, ()), None);
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(63, 127), &target, ()).unwrap(),
                    abs_con(0.49606299212598426)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(127, 127), &target, ()).unwrap(),
                    abs_con(1.0)
                );
            }

            #[test]
            fn default_with_virtual_target() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.3779527559055118)),
                    control_type: ControlType::VirtualMulti,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(0, 127), &target, ()).unwrap(),
                    abs_dis(0, 127)
                );
                assert_eq!(mode.control(abs_con(0.3779527559055118), &target, ()), None);
                assert_eq!(mode.control(abs_dis(48, 127), &target, ()), None);
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(63, 127), &target, ()).unwrap(),
                    abs_dis(63, 127)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(127, 127), &target, ()).unwrap(),
                    abs_dis(127, 127)
                );
            }

            #[test]
            fn default_target_is_trigger() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuousRetriggerable,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.777), &target, ()).unwrap(),
                    abs_con(0.777)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
            }

            #[test]
            fn relative_target() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: None,
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
            }

            #[test]
            fn source_interval() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    source_value_interval: create_unit_value_interval(0.2, 0.6),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(0, 127), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(13, 127), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.2), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(25, 127), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.4), &target, ()).unwrap(),
                    abs_con(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(51, 127), &target, ()).unwrap(),
                    abs_con(0.5039370078740157)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.6), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(76, 127), &target, ()).unwrap(),
                    abs_con(0.9960629921259844)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.8), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(80, 127), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(127, 127), &target, ()).unwrap(),
                    abs_con(1.0)
                );
            }

            #[test]
            fn source_interval_out_of_range_ignore() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    source_value_interval: create_unit_value_interval(0.2, 0.6),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.1), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.2), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.4), &target, ()).unwrap(),
                    abs_con(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.6), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert!(mode.control(abs_con(0.8), &target, ()).is_none());
                assert!(mode.control(abs_con(1.0), &target, ()).is_none());
            }

            #[test]
            fn source_interval_out_of_range_min() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    source_value_interval: create_unit_value_interval(0.2, 0.6),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.2), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.4), &target, ()).unwrap(),
                    abs_con(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.6), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.8), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
            }

            #[test]
            fn source_interval_out_of_range_ignore_source_one_value() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    source_value_interval: create_unit_value_interval(0.5, 0.5),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.4), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert!(mode.control(abs_con(0.6), &target, ()).is_none());
                assert!(mode.control(abs_con(1.0), &target, ()).is_none());
            }

            #[test]
            fn source_interval_out_of_range_min_source_one_value() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    source_value_interval: create_unit_value_interval(0.5, 0.5),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.4), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.6), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
            }

            #[test]
            fn source_interval_out_of_range_min_max_source_one_value() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    source_value_interval: create_unit_value_interval(0.5, 0.5),
                    out_of_range_behavior: OutOfRangeBehavior::MinOrMax,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.4), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.6), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
            }

            #[test]
            fn target_interval() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.2, 0.6),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.2), &target, ()).unwrap(),
                    abs_con(0.28)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.25), &target, ()).unwrap(),
                    abs_con(0.3)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.75), &target, ()).unwrap(),
                    abs_con(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.6)
                );
            }

            #[test]
            fn target_interval_reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.6, 1.0),
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.25), &target, ()).unwrap(),
                    abs_con(0.9)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.75), &target, ()).unwrap(),
                    abs_con(0.7)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.6)
                );
            }

            #[test]
            fn source_and_target_interval() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    source_value_interval: create_unit_value_interval(0.2, 0.6),
                    target_value_interval: create_unit_value_interval(0.2, 0.6),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.2), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.4), &target, ()).unwrap(),
                    abs_con(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.6), &target, ()).unwrap(),
                    abs_con(0.6)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.8), &target, ()).unwrap(),
                    abs_con(0.6)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.6)
                );
            }

            #[test]
            fn source_and_target_interval_shifted() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    source_value_interval: create_unit_value_interval(0.2, 0.6),
                    target_value_interval: create_unit_value_interval(0.4, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.2), &target, ()).unwrap(),
                    abs_con(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.4), &target, ()).unwrap(),
                    abs_con(0.6)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.6), &target, ()).unwrap(),
                    abs_con(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.8), &target, ()).unwrap(),
                    abs_con(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.8)
                );
            }

            #[test]
            fn reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
            }

            #[test]
            fn reverse_discrete_target() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(55, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                // Check that we use a "scaling reverse" instead of a "subtracting" reverse even
                // though control value and target is discrete. Discrete processing is disabled!
                assert_abs_diff_eq!(
                    mode.control(abs_dis(0, 10), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(5, 10), &target, ()).unwrap(),
                    abs_con(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(10, 10), &target, ()).unwrap(),
                    abs_con(0.0)
                );
            }

            #[test]
            fn discrete_target() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(4, 5)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.2),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_eq!(mode.control(abs_con(0.8), &target, ()), None);
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.19), &target, ()).unwrap(),
                    abs_con(0.19)
                );

                assert_abs_diff_eq!(
                    mode.control(abs_con(0.49), &target, ()).unwrap(),
                    abs_con(0.49)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
            }

            #[test]
            fn round() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    round_target_value: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(4, 5)),
                    control_type: ControlType::AbsoluteContinuousRoundable {
                        rounding_step_size: UnitValue::new(0.2),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.11), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.19), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.2), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.35), &target, ()).unwrap(),
                    abs_con(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.49), &target, ()).unwrap(),
                    abs_con(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
            }

            #[test]
            fn jump_interval() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    jump_interval: create_unit_value_interval(0.0, 0.2),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.5)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.1), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.4), &target, ()).unwrap(),
                    abs_con(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.6), &target, ()).unwrap(),
                    abs_con(0.6)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.7), &target, ()).unwrap(),
                    abs_con(0.7)
                );
                assert!(mode.control(abs_con(0.8), &target, ()).is_none());
                assert!(mode.control(abs_con(0.9), &target, ()).is_none());
                assert!(mode.control(abs_con(1.0), &target, ()).is_none());
            }

            #[test]
            fn jump_interval_min() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    jump_interval: create_unit_value_interval(0.1, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.5)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.1)
                );
                assert!(mode.control(abs_con(0.4), &target, ()).is_none());
                assert!(mode.control(abs_con(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con(0.6), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
            }

            #[test]
            fn jump_interval_approach() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    jump_interval: create_unit_value_interval(0.0, 0.2),
                    takeover_mode: TakeoverMode::LongTimeNoSee,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.5)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.42)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.4), &target, ()).unwrap(),
                    abs_con(0.4)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.6), &target, ()).unwrap(),
                    abs_con(0.6)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.7), &target, ()).unwrap(),
                    abs_con(0.7)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.8), &target, ()).unwrap(),
                    abs_con(0.56)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.6)
                );
            }

            #[test]
            fn transformation_ok() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    control_transformation: Some(TestTransformation::new(|input| Ok(1.0 - input))),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
            }

            #[test]
            fn transformation_err() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    control_transformation: Some(TestTransformation::new(|_| Err("oh no!"))),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.777)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
            }

            #[test]
            fn feedback() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(dis_val(0, 127)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(
                    mode.feedback(dis_val(60, 127)).unwrap(),
                    con_val(0.47244094488188976)
                );
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(dis_val(127, 127)).unwrap(), con_val(1.0));
            }

            #[test]
            fn feedback_with_virtual_source() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    ..Default::default()
                };
                let options = ModeFeedbackOptions {
                    source_is_virtual: true,
                    max_discrete_source_value: None,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.feedback_with_options(con_val(0.0), options).unwrap(),
                    con_val(0.0)
                );
                assert_abs_diff_eq!(
                    mode.feedback_with_options(dis_val(0, 127), options)
                        .unwrap(),
                    dis_val(0, 127)
                );
                assert_abs_diff_eq!(
                    mode.feedback_with_options(con_val(0.5), options).unwrap(),
                    con_val(0.5)
                );
                assert_abs_diff_eq!(
                    mode.feedback_with_options(dis_val(60, 127), options)
                        .unwrap(),
                    dis_val(60, 127)
                );
                assert_abs_diff_eq!(
                    mode.feedback_with_options(con_val(1.0), options).unwrap(),
                    con_val(1.0)
                );
                assert_abs_diff_eq!(
                    mode.feedback_with_options(dis_val(127, 127), options)
                        .unwrap(),
                    dis_val(127, 127)
                );
            }

            #[test]
            fn feedback_reverse() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    reverse: true,
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.0));
            }

            #[test]
            fn feedback_target_interval() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.2)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.4)).unwrap(), con_val(0.25));
                assert_abs_diff_eq!(mode.feedback(con_val(0.6)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(0.8)).unwrap(), con_val(0.75));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(1.0));
            }

            #[test]
            fn feedback_target_interval_reverse() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.2, 1.0),
                    reverse: true,
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.2)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.4)).unwrap(), con_val(0.75));
                assert_abs_diff_eq!(mode.feedback(con_val(0.6)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(0.8)).unwrap(), con_val(0.25));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.0));
            }

            #[test]
            fn feedback_source_and_target_interval() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    source_value_interval: create_unit_value_interval(0.2, 0.8),
                    target_value_interval: create_unit_value_interval(0.4, 1.0),
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.2));
                assert_abs_diff_eq!(mode.feedback(con_val(0.4)).unwrap(), con_val(0.2));
                assert_abs_diff_eq!(mode.feedback(con_val(0.7)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.8));
            }

            #[test]
            fn feedback_out_of_range_ignore() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                };
                // When
                // Then
                assert!(mode.feedback(con_val(0.0)).is_none());
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
                assert!(mode.feedback(con_val(1.0)).is_none());
            }

            #[test]
            fn feedback_out_of_range_min() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.1)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(0.9)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.0));
            }

            #[test]
            fn feedback_out_of_range_min_max_okay() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.02, 0.02),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.01)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.02)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.03)).unwrap(), con_val(0.0));
            }

            #[test]
            fn feedback_out_of_range_min_max_issue_263() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.03, 0.03),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.01)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.03)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.04)).unwrap(), con_val(0.0));
            }

            #[test]
            fn feedback_out_of_range_min_max_issue_263_more() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.03, 0.03),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.01)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(
                    mode.feedback(con_val(0.029999999329447746)).unwrap(),
                    con_val(1.0)
                );
                assert_abs_diff_eq!(mode.feedback(con_val(0.0300000001)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.04)).unwrap(), con_val(0.0));
            }

            #[test]
            fn feedback_out_of_range_min_target_one_value() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.5, 0.5),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.1)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.9)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.0));
            }

            #[test]
            fn feedback_out_of_range_min_max_target_one_value() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.5, 0.5),
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.1)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.9)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(1.0));
            }

            #[test]
            fn feedback_out_of_range_ignore_target_one_value() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.5, 0.5),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                };
                // When
                // Then
                assert!(mode.feedback(con_val(0.0)).is_none());
                assert!(mode.feedback(con_val(0.1)).is_none());
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(1.0));
                assert!(mode.feedback(con_val(0.9)).is_none());
                assert!(mode.feedback(con_val(1.0)).is_none());
            }

            #[test]
            fn feedback_transformation() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    feedback_transformation: Some(TestTransformation::new(|input| Ok(1.0 - input))),
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.0));
            }
        }

        mod discrete_processing {
            use super::*;

            #[test]
            fn case_1_no_interval_restriction() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(48, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(0, 100), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(mode.control(abs_dis(48, 100), &target, ()), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(48, 200), FB_OPTS),
                    Some(dis_val(48, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(50, 100), &target, ()).unwrap(),
                    abs_dis(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 100), &target, ()).unwrap(),
                    abs_dis(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
            }

            #[test]
            fn case_2_target_interval_min() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(50, u32::MAX),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(98, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(0, 100), &target, ()).unwrap(),
                    abs_dis(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(mode.control(abs_dis(48, 100), &target, ()), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(98, 200), FB_OPTS),
                    Some(dis_val(48, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(50, 100), &target, ()).unwrap(),
                    abs_dis(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 100), &target, ()).unwrap(),
                    abs_dis(150, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
            }

            #[test]
            fn case_3_source_interval_min() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(50, u32::MAX),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(48, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(50, 100), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(mode.control(abs_dis(98, 100), &target, ()), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(48, 200), FB_OPTS),
                    Some(dis_val(98, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 100), &target, ()).unwrap(),
                    abs_dis(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.control(abs_dis(0, 100), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
            }

            #[test]
            fn case_4_source_and_target_interval_min() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(50, u32::MAX),
                    discrete_target_value_interval: Interval::new(50, u32::MAX),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(98, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(50, 100), &target, ()).unwrap(),
                    abs_dis(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(mode.control(abs_dis(98, 100), &target, ()), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(98, 200), FB_OPTS),
                    Some(dis_val(98, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 100), &target, ()).unwrap(),
                    abs_dis(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.control(abs_dis(0, 100), &target, ()).unwrap(),
                    abs_dis(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
            }

            #[test]
            fn case_5_source_and_target_interval_min_max() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(0, 50),
                    discrete_target_value_interval: Interval::new(50, u32::MAX),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(98, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(0, 100), &target, ()).unwrap(),
                    abs_dis(50, 200)
                );
                assert_eq!(mode.control(abs_dis(48, 100), &target, ()), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(98, 200), FB_OPTS),
                    Some(dis_val(48, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(50, 100), &target, ()).unwrap(),
                    abs_dis(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(mode.control(abs_dis(48, 100), &target, ()), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(98, 200), FB_OPTS),
                    Some(dis_val(48, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 100), &target, ()).unwrap(),
                    abs_dis(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
            }

            #[test]
            fn case_6_source_and_target_interval_disjoint() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(50, u32::MAX),
                    discrete_target_value_interval: Interval::new(200, u32::MAX),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(248, 350)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 350.0),
                    },
                };
                let fb_opts = ModeFeedbackOptions {
                    source_is_virtual: false,
                    // Count: 151
                    max_discrete_source_value: Some(150),
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(50, 150), &target, ()).unwrap(),
                    abs_dis(200, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 350), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(mode.control(abs_dis(98, 150), &target, ()), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(248, 350), fb_opts),
                    Some(dis_val(98, 150))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 150), &target, ()).unwrap(),
                    abs_dis(250, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(250, 350), fb_opts),
                    Some(dis_val(100, 150))
                );
                assert_eq!(
                    mode.control(abs_dis(150, 150), &target, ()).unwrap(),
                    abs_dis(300, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(300, 350), fb_opts),
                    Some(dis_val(150, 150))
                );
                // Out of range
                assert_eq!(
                    mode.control(abs_dis(0, 150), &target, ()).unwrap(),
                    abs_dis(200, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 350), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 350), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 350), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 350), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(350, 350), fb_opts),
                    Some(dis_val(150, 150))
                );
            }

            #[test]
            fn case_7_interval_max_greater_than_target_max() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(50, 200),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(98, 150)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 150.0),
                    },
                };
                let fb_opts = ModeFeedbackOptions {
                    source_is_virtual: false,
                    // Count: 151
                    max_discrete_source_value: Some(150),
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(0, 150), &target, ()).unwrap(),
                    abs_dis(50, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 150), fb_opts),
                    Some(dis_val(0, 150))
                );
                assert_eq!(
                    mode.control(abs_dis(50, 150), &target, ()).unwrap(),
                    abs_dis(100, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 150), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 150), &target, ()).unwrap(),
                    abs_dis(150, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 150), fb_opts),
                    Some(dis_val(100, 150))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 150), fb_opts),
                    Some(dis_val(0, 150))
                );
                assert_eq!(
                    mode.control(abs_dis(150, 150), &target, ()).unwrap(),
                    abs_dis(150, 150)
                );
            }

            #[test]
            fn case_8_target_subset_interval() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(50, 100),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(98, 150)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 150.0),
                    },
                };
                let fb_opts = ModeFeedbackOptions {
                    source_is_virtual: false,
                    // Count: 151
                    max_discrete_source_value: Some(150),
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(0, 150), &target, ()).unwrap(),
                    abs_dis(50, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 150), fb_opts),
                    Some(dis_val(0, 150))
                );
                assert_eq!(
                    mode.control(abs_dis(50, 150), &target, ()).unwrap(),
                    abs_dis(100, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 150), fb_opts),
                    Some(dis_val(50, 150))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 150), fb_opts),
                    Some(dis_val(0, 150))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 150), &target, ()).unwrap(),
                    abs_dis(100, 150)
                );
                assert_eq!(
                    mode.control(abs_dis(150, 150), &target, ()).unwrap(),
                    abs_dis(100, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 150), fb_opts),
                    Some(dis_val(50, 150))
                );
            }

            #[test]
            fn case_9_no_interval_restriction_reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(48, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(0, 100), &target, ()).unwrap(),
                    abs_dis(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(50, 100), &target, ()).unwrap(),
                    abs_dis(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 100), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
            }

            #[test]
            fn case_10_target_interval_min_reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    reverse: true,
                    discrete_target_value_interval: Interval::new(50, u32::MAX),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(98, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(0, 100), &target, ()).unwrap(),
                    abs_dis(150, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(50, 100), &target, ()).unwrap(),
                    abs_dis(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 100), &target, ()).unwrap(),
                    abs_dis(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(0, 100))
                );
            }

            #[test]
            fn case_11_source_interval_min_reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    reverse: true,
                    discrete_source_value_interval: Interval::new(50, u32::MAX),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(48, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(50, 100), &target, ()).unwrap(),
                    abs_dis(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 100), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.control(abs_dis(0, 100), &target, ()).unwrap(),
                    abs_dis(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
            }

            #[test]
            fn case_12_source_and_target_interval_min_reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    reverse: true,
                    discrete_source_value_interval: Interval::new(50, u32::MAX),
                    discrete_target_value_interval: Interval::new(50, u32::MAX),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(98, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(50, 100), &target, ()).unwrap(),
                    abs_dis(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 100), &target, ()).unwrap(),
                    abs_dis(50, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                // Out of range
                assert_eq!(
                    mode.control(abs_dis(0, 100), &target, ()).unwrap(),
                    abs_dis(100, 200)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS),
                    Some(dis_val(100, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 200), FB_OPTS),
                    Some(dis_val(50, 100))
                );
            }

            #[test]
            fn case_13_source_and_target_interval_disjoint_reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    reverse: true,
                    discrete_source_value_interval: Interval::new(50, u32::MAX),
                    discrete_target_value_interval: Interval::new(200, u32::MAX),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(248, 350)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 350.0),
                    },
                };
                let fb_opts = ModeFeedbackOptions {
                    source_is_virtual: false,
                    // Count: 151
                    max_discrete_source_value: Some(150),
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(50, 150), &target, ()).unwrap(),
                    abs_dis(300, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(300, 350), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 150), &target, ()).unwrap(),
                    abs_dis(250, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(250, 350), fb_opts),
                    Some(dis_val(100, 150))
                );
                assert_eq!(
                    mode.control(abs_dis(150, 150), &target, ()).unwrap(),
                    abs_dis(200, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(200, 350), fb_opts),
                    Some(dis_val(150, 150))
                );
                // Out of range
                assert_eq!(
                    mode.control(abs_dis(0, 150), &target, ()).unwrap(),
                    abs_dis(300, 350)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 350), fb_opts),
                    Some(dis_val(150, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 350), fb_opts),
                    Some(dis_val(150, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 350), fb_opts),
                    Some(dis_val(150, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 350), fb_opts),
                    Some(dis_val(150, 150))
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(350, 350), fb_opts),
                    Some(dis_val(50, 150))
                );
            }

            #[test]
            fn case_14_interval_max_greater_than_target_max_reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    reverse: true,
                    discrete_target_value_interval: Interval::new(50, 200),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(98, 150)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 150.0),
                    },
                };
                let fb_opts = ModeFeedbackOptions {
                    source_is_virtual: false,
                    // Count: 151
                    max_discrete_source_value: Some(150),
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(0, 150), &target, ()).unwrap(),
                    abs_dis(150, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 150), fb_opts),
                    Some(dis_val(0, 150))
                );
                assert_eq!(
                    mode.control(abs_dis(50, 150), &target, ()).unwrap(),
                    abs_dis(100, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 150), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 150), &target, ()).unwrap(),
                    abs_dis(50, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 150), fb_opts),
                    Some(dis_val(100, 150))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 150), fb_opts),
                    Some(dis_val(100, 150))
                );
                assert_eq!(
                    mode.control(abs_dis(150, 150), &target, ()).unwrap(),
                    abs_dis(50, 150)
                );
            }

            #[test]
            fn case_15_target_subset_interval_reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    reverse: true,
                    discrete_target_value_interval: Interval::new(50, 100),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(98, 150)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 150.0),
                    },
                };
                let fb_opts = ModeFeedbackOptions {
                    source_is_virtual: false,
                    // Count: 151
                    max_discrete_source_value: Some(150),
                };
                // When
                // Then
                // Within range
                assert_eq!(
                    mode.control(abs_dis(0, 150), &target, ()).unwrap(),
                    abs_dis(100, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 150), fb_opts),
                    Some(dis_val(0, 150))
                );
                assert_eq!(
                    mode.control(abs_dis(50, 150), &target, ()).unwrap(),
                    abs_dis(50, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 150), fb_opts),
                    Some(dis_val(50, 150))
                );
                // Out of range
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 150), fb_opts),
                    Some(dis_val(50, 150))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 150), &target, ()).unwrap(),
                    abs_dis(50, 150)
                );
                assert_eq!(
                    mode.control(abs_dis(150, 150), &target, ()).unwrap(),
                    abs_dis(50, 150)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(150, 150), fb_opts),
                    Some(dis_val(0, 150))
                );
            }

            #[test]
            fn default_with_virtual_target() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: None,
                    control_type: ControlType::VirtualMulti,
                };
                // When
                // Then
                assert_eq!(
                    mode.control(abs_dis(0, 100), &target, ()).unwrap(),
                    abs_dis(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS),
                    Some(dis_val(0, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(63, 100), &target, ()).unwrap(),
                    abs_dis(63, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(63, 100), FB_OPTS),
                    Some(dis_val(63, 100))
                );
                assert_eq!(
                    mode.control(abs_dis(100, 100), &target, ()).unwrap(),
                    abs_dis(100, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 100), FB_OPTS),
                    Some(dis_val(100, 100))
                );
            }

            #[test]
            fn relative_target() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: None,
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis(0, 127), &target, ()).unwrap(),
                    abs_dis(0, 127)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(63, 127), &target, ()).unwrap(),
                    abs_dis(63, 127)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(127, 127), &target, ()).unwrap(),
                    abs_dis(127, 127)
                );
            }

            #[test]
            fn source_interval_out_of_range_ignore() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(20, 60),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(48, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_dis(0, 127), &target, ()).is_none());
                assert!(mode.control(abs_dis(19, 127), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_dis(20, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(40, 127), &target, ()).unwrap(),
                    abs_dis(20, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(60, 127), &target, ()).unwrap(),
                    abs_dis(40, 200)
                );
                assert!(mode.control(abs_dis(70, 127), &target, ()).is_none());
                assert!(mode.control(abs_dis(100, 127), &target, ()).is_none());
            }

            #[test]
            fn source_interval_out_of_range_min() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(20, 60),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis(0, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(1, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(20, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(40, 127), &target, ()).unwrap(),
                    abs_dis(20, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(60, 127), &target, ()).unwrap(),
                    abs_dis(40, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(90, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(127, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
            }

            #[test]
            fn source_interval_out_of_range_ignore_source_one_value() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(60, 60),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_dis(0, 127), &target, ()).is_none());
                assert!(mode.control(abs_dis(59, 127), &target, ()).is_none());
                // TODO-high-discrete Not sure if this abs_dis(1, 1) is serving the actual use case
                //  here and in following tests...
                assert_eq!(
                    mode.control(abs_dis(60, 127), &target, ()).unwrap(),
                    abs_dis(1, 200)
                );
                assert!(mode.control(abs_dis(61, 127), &target, ()).is_none());
                assert!(mode.control(abs_dis(127, 127), &target, ()).is_none());
            }

            #[test]
            fn source_interval_out_of_range_min_source_one_value() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(60, 60),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis(0, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(59, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(60, 127), &target, ()).unwrap(),
                    abs_dis(1, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(61, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(61, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
            }

            #[test]
            fn source_interval_out_of_range_min_max_source_one_value() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(60, 60),
                    out_of_range_behavior: OutOfRangeBehavior::MinOrMax,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis(0, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(59, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(60, 127), &target, ()).unwrap(),
                    abs_dis(1, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(61, 127), &target, ()).unwrap(),
                    abs_dis(1, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(127, 127), &target, ()).unwrap(),
                    abs_dis(1, 200)
                );
            }

            #[test]
            fn target_interval_reverse() {
                // TODO-high-discrete Add other reverse tests with source and target interval and
                //  also intervals with max values! First for continuous!
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(70, 100),
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis(0, 127), &target, ()).unwrap(),
                    abs_dis(100, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(1, 127), &target, ()).unwrap(),
                    abs_dis(99, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(15, 127), &target, ()).unwrap(),
                    abs_dis(85, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(30, 100), &target, ()).unwrap(),
                    abs_dis(70, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(40, 100), &target, ()).unwrap(),
                    abs_dis(70, 200)
                );
            }

            #[test]
            fn reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis(0, 127), &target, ()).unwrap(),
                    abs_dis(127, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(63, 127), &target, ()).unwrap(),
                    abs_dis(127 - 63, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(127, 127), &target, ()).unwrap(),
                    abs_dis(127 - 127, 200)
                );
            }

            #[test]
            fn round() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    round_target_value: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(2, 5)),
                    control_type: ControlType::AbsoluteContinuousRoundable {
                        rounding_step_size: UnitValue::new(0.2),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis(0, 127), &target, ()).unwrap(),
                    abs_dis(0, 5)
                );
                assert_eq!(mode.control(abs_dis(2, 127), &target, ()), None);
                assert_abs_diff_eq!(
                    mode.control(abs_dis(3, 127), &target, ()).unwrap(),
                    abs_dis(3, 5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(127, 127), &target, ()).unwrap(),
                    abs_dis(5, 5)
                );
            }

            #[test]
            fn jump_interval() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_jump_interval: Interval::new(0, 2),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(60, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                assert_eq!(mode.control(abs_dis(0, 127), &target, ()), None);
                assert_eq!(mode.control(abs_dis(57, 127), &target, ()), None);
                assert_abs_diff_eq!(
                    mode.control(abs_dis(58, 127), &target, ()).unwrap(),
                    abs_dis(58, 200)
                );
                assert_eq!(mode.control(abs_dis(60, 127), &target, ()), None);
                assert_abs_diff_eq!(
                    mode.control(abs_dis(61, 127), &target, ()).unwrap(),
                    abs_dis(61, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(62, 127), &target, ()).unwrap(),
                    abs_dis(62, 200)
                );
                assert!(mode.control(abs_dis(63, 200), &target, ()).is_none());
                assert!(mode.control(abs_dis(127, 200), &target, ()).is_none());
            }

            #[test]
            fn jump_interval_min() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_jump_interval: Interval::new(10, 100),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(60, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis(1, 127), &target, ()).unwrap(),
                    abs_dis(1, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(50, 127), &target, ()).unwrap(),
                    abs_dis(50, 200)
                );
                assert!(mode.control(abs_dis(55, 127), &target, ()).is_none());
                assert!(mode.control(abs_dis(65, 127), &target, ()).is_none());
                assert!(mode.control(abs_dis(69, 127), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_dis(70, 127), &target, ()).unwrap(),
                    abs_dis(70, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(127, 127), &target, ()).unwrap(),
                    abs_dis(127, 200)
                );
            }

            // #[test]
            // fn jump_interval_approach() {
            //     // Given
            //     let mut mode: Mode<TestTransformation> = Mode {
            //         jump_interval: create_unit_value_interval(0.0, 0.2),
            //         takeover_mode: TakeoverMode::LongTimeNoSee,
            //         ..Default::default()
            //     };
            //     let target = TestTarget {
            //         current_value: Some(continuous_value(0.5)),
            //         control_type: ControlType::AbsoluteContinuous,
            //     };
            //     // When
            //     // Then
            //     assert_abs_diff_eq!(
            //         mode.control(abs_con(0.0), &target, ()).unwrap(),
            //         abs_con(0.4)
            //     );
            //     assert_abs_diff_eq!(
            //         mode.control(abs_con(0.1), &target, ()).unwrap(),
            //         abs_con(0.42)
            //     );
            //     assert_abs_diff_eq!(
            //         mode.control(abs_con(0.4), &target, ()).unwrap(),
            //         abs_con(0.4)
            //     );
            //     assert_abs_diff_eq!(
            //         mode.control(abs_con(0.6), &target, ()).unwrap(),
            //         abs_con(0.6)
            //     );
            //     assert_abs_diff_eq!(
            //         mode.control(abs_con(0.7), &target, ()).unwrap(),
            //         abs_con(0.7)
            //     );
            //     assert_abs_diff_eq!(
            //         mode.control(abs_con(0.8), &target, ()).unwrap(),
            //         abs_con(0.56)
            //     );
            //     assert_abs_diff_eq!(
            //         mode.control(abs_con(1.0), &target, ()).unwrap(),
            //         abs_con(0.6)
            //     );
            // }

            #[test]
            fn transformation_ok() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    control_transformation: Some(TestTransformation::new(|input| Ok(input + 20.0))),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis(0, 127), &target, ()).unwrap(),
                    abs_dis(20, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(60, 127), &target, ()).unwrap(),
                    abs_dis(80, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(120, 127), &target, ()).unwrap(),
                    abs_dis(140, 200)
                );
            }

            #[test]
            fn transformation_negative() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    control_transformation: Some(TestTransformation::new(|input| Ok(input - 20.0))),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis(0, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(20, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(120, 127), &target, ()).unwrap(),
                    abs_dis(100, 200)
                );
            }

            #[test]
            fn transformation_err() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    control_transformation: Some(TestTransformation::new(|_| Err("oh no!"))),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(dis_val(38, 200)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(1.0 / 200.0),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(
                    mode.control(abs_dis(0, 127), &target, ()).unwrap(),
                    abs_dis(0, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(60, 127), &target, ()).unwrap(),
                    abs_dis(60, 200)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_dis(127, 127), &target, ()).unwrap(),
                    abs_dis(127, 200)
                );
            }

            #[test]
            fn feedback() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    ..Default::default()
                };
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 10), FB_OPTS).unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(5, 10), FB_OPTS).unwrap(),
                    dis_val(5, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(10, 10), FB_OPTS)
                        .unwrap(),
                    dis_val(10, 100)
                );
            }

            #[test]
            fn feedback_with_virtual_source() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    ..Default::default()
                };
                let options = ModeFeedbackOptions {
                    source_is_virtual: true,
                    max_discrete_source_value: None,
                };
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 10), options).unwrap(),
                    dis_val(0, 10)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(5, 10), options).unwrap(),
                    dis_val(5, 10)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(10, 10), options)
                        .unwrap(),
                    dis_val(10, 10)
                );
            }

            #[test]
            fn feedback_reverse() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    reverse: true,
                    ..Default::default()
                };
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 127), FB_OPTS)
                        .unwrap(),
                    dis_val(100, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(5, 127), FB_OPTS)
                        .unwrap(),
                    dis_val(100 - 5, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(10, 127), FB_OPTS)
                        .unwrap(),
                    dis_val(100 - 10, 100)
                );
            }

            #[test]
            fn feedback_target_interval() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(20, u32::MAX),
                    ..Default::default()
                };
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(20, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(40, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(20, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(60, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(40, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(80, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(60, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(80, 100)
                );
            }

            #[test]
            fn feedback_target_interval_reverse() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(20, u32::MAX),
                    reverse: true,
                    ..Default::default()
                };
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 200), FB_OPTS)
                        .unwrap(),
                    dis_val(100, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(20, 200), FB_OPTS)
                        .unwrap(),
                    dis_val(100, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(40, 200), FB_OPTS)
                        .unwrap(),
                    dis_val(100 - 20, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(60, 200), FB_OPTS)
                        .unwrap(),
                    dis_val(100 - 40, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(80, 200), FB_OPTS)
                        .unwrap(),
                    dis_val(100 - 60, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 200), FB_OPTS)
                        .unwrap(),
                    dis_val(100 - 80, 100)
                );
            }

            #[test]
            fn feedback_source_and_target_interval() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_source_value_interval: Interval::new(20, 80),
                    discrete_target_value_interval: Interval::new(40, 100),
                    ..Default::default()
                };
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(20, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(40, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(20, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(70, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(50, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(80, 100)
                );
            }

            #[test]
            fn feedback_out_of_range_ignore() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(20, 80),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                };
                // When
                // Then
                assert_eq!(mode.feedback_with_options(dis_val(0, 100), FB_OPTS), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(30, 100)
                );
                assert_eq!(mode.feedback_with_options(dis_val(100, 100), FB_OPTS), None);
            }

            #[test]
            fn feedback_out_of_range_min() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(20, 80),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                };
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(10, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(30, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(90, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
            }

            #[test]
            fn feedback_out_of_range_min_max_okay() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(2, 2),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                };
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(1, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                // TODO-high-discrete Would make more sense to use SOURCE MAX instead of 1. So this
                //  (1, 1) thing is not that useful! We should have a way to express MAX - which is
                //  u32:MAX. It should then be clamped to
                //  min(source_interval_max, discrete_source_max).
                assert_eq!(
                    mode.feedback_with_options(dis_val(2, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(1, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(3, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
            }

            #[test]
            fn feedback_out_of_range_min_max_issue_263() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(3, 3),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                };
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(1, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(2, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(3, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(1, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(4, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
            }

            #[test]
            fn feedback_out_of_range_min_target_one_value() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(50, 50),
                    out_of_range_behavior: OutOfRangeBehavior::Min,
                    ..Default::default()
                };
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(10, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(1, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(90, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
            }

            #[test]
            fn feedback_out_of_range_min_max_target_one_value() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(50, 50),
                    ..Default::default()
                };
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(0, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(10, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(1, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(90, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(1, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(1, 100)
                );
            }

            #[test]
            fn feedback_out_of_range_ignore_target_one_value() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    discrete_target_value_interval: Interval::new(50, 50),
                    out_of_range_behavior: OutOfRangeBehavior::Ignore,
                    ..Default::default()
                };
                // When
                // Then
                assert_eq!(mode.feedback_with_options(dis_val(0, 100), FB_OPTS), None);
                assert_eq!(mode.feedback_with_options(dis_val(10, 100), FB_OPTS), None);
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 100), FB_OPTS)
                        .unwrap(),
                    dis_val(1, 100)
                );
                assert_eq!(mode.feedback_with_options(dis_val(90, 100), FB_OPTS), None);
                assert_eq!(mode.feedback_with_options(dis_val(100, 100), FB_OPTS), None);
            }

            #[test]
            fn feedback_transformation() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    use_discrete_processing: true,
                    feedback_transformation: Some(TestTransformation::new(
                        |input| Ok(input - 10.0),
                    )),
                    ..Default::default()
                };
                // When
                // Then
                assert_eq!(
                    mode.feedback_with_options(dis_val(00, 127), FB_OPTS)
                        .unwrap(),
                    dis_val(0, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(50, 127), FB_OPTS)
                        .unwrap(),
                    dis_val(40, 100)
                );
                assert_eq!(
                    mode.feedback_with_options(dis_val(100, 127), FB_OPTS)
                        .unwrap(),
                    dis_val(90, 100)
                );
            }
        }
    }

    mod absolute_toggle {
        use super::*;

        #[test]
        fn absolute_value_target_off() {
            // Given
            let mut mode: Mode<TestTransformation> = Mode {
                absolute_mode: AbsoluteMode::ToggleButtons,
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(con_val(0.0)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con(0.1), &target, ()).unwrap(),
                abs_con(1.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(0.5), &target, ()).unwrap(),
                abs_con(1.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(1.0), &target, ()).unwrap(),
                abs_con(1.0)
            );
        }

        #[test]
        fn absolute_value_target_on() {
            // Given
            let mut mode: Mode<TestTransformation> = Mode {
                absolute_mode: AbsoluteMode::ToggleButtons,
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(con_val(1.0)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con(0.1), &target, ()).unwrap(),
                abs_con(0.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(0.5), &target, ()).unwrap(),
                abs_con(0.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(1.0), &target, ()).unwrap(),
                abs_con(0.0)
            );
        }

        #[test]
        fn absolute_value_target_rather_off() {
            // Given
            let mut mode: Mode<TestTransformation> = Mode {
                absolute_mode: AbsoluteMode::ToggleButtons,
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(con_val(0.333)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con(0.1), &target, ()).unwrap(),
                abs_con(1.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(0.5), &target, ()).unwrap(),
                abs_con(1.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(1.0), &target, ()).unwrap(),
                abs_con(1.0)
            );
        }

        #[test]
        fn absolute_value_target_rather_on() {
            // Given
            let mut mode: Mode<TestTransformation> = Mode {
                absolute_mode: AbsoluteMode::ToggleButtons,
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(con_val(0.777)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con(0.1), &target, ()).unwrap(),
                abs_con(0.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(0.5), &target, ()).unwrap(),
                abs_con(0.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(1.0), &target, ()).unwrap(),
                abs_con(0.0)
            );
        }

        #[test]
        fn absolute_value_target_interval_target_off() {
            // Given
            let mut mode: Mode<TestTransformation> = Mode {
                absolute_mode: AbsoluteMode::ToggleButtons,
                target_value_interval: create_unit_value_interval(0.3, 0.7),
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(con_val(0.3)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con(0.1), &target, ()).unwrap(),
                abs_con(0.7)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(0.5), &target, ()).unwrap(),
                abs_con(0.7)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(1.0), &target, ()).unwrap(),
                abs_con(0.7)
            );
        }

        #[test]
        fn absolute_value_target_interval_target_on() {
            // Given
            let mut mode: Mode<TestTransformation> = Mode {
                absolute_mode: AbsoluteMode::ToggleButtons,
                target_value_interval: create_unit_value_interval(0.3, 0.7),
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(con_val(0.7)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con(0.1), &target, ()).unwrap(),
                abs_con(0.3)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(0.5), &target, ()).unwrap(),
                abs_con(0.3)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(1.0), &target, ()).unwrap(),
                abs_con(0.3)
            );
        }

        #[test]
        fn absolute_value_target_interval_target_rather_off() {
            // Given
            let mut mode: Mode<TestTransformation> = Mode {
                absolute_mode: AbsoluteMode::ToggleButtons,
                target_value_interval: create_unit_value_interval(0.3, 0.7),
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(con_val(0.4)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con(0.1), &target, ()).unwrap(),
                abs_con(0.7)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(0.5), &target, ()).unwrap(),
                abs_con(0.7)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(1.0), &target, ()).unwrap(),
                abs_con(0.7)
            );
        }

        #[test]
        fn absolute_value_target_interval_target_rather_on() {
            // Given
            let mut mode: Mode<TestTransformation> = Mode {
                absolute_mode: AbsoluteMode::ToggleButtons,
                target_value_interval: create_unit_value_interval(0.3, 0.7),
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(con_val(0.6)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con(0.1), &target, ()).unwrap(),
                abs_con(0.3)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(0.5), &target, ()).unwrap(),
                abs_con(0.3)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(1.0), &target, ()).unwrap(),
                abs_con(0.3)
            );
        }

        #[test]
        fn absolute_value_target_interval_target_too_off() {
            // Given
            let mut mode: Mode<TestTransformation> = Mode {
                absolute_mode: AbsoluteMode::ToggleButtons,
                target_value_interval: create_unit_value_interval(0.3, 0.7),
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(con_val(0.0)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con(0.1), &target, ()).unwrap(),
                abs_con(0.7)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(0.5), &target, ()).unwrap(),
                abs_con(0.7)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(1.0), &target, ()).unwrap(),
                abs_con(0.7)
            );
        }

        #[test]
        fn absolute_value_target_interval_target_too_on() {
            // Given
            let mut mode: Mode<TestTransformation> = Mode {
                absolute_mode: AbsoluteMode::ToggleButtons,
                target_value_interval: create_unit_value_interval(0.3, 0.7),
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(con_val(1.0)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert!(mode.control(abs_con(0.0), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_con(0.1), &target, ()).unwrap(),
                abs_con(0.3)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(0.5), &target, ()).unwrap(),
                abs_con(0.3)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(1.0), &target, ()).unwrap(),
                abs_con(0.3)
            );
        }

        #[test]
        fn feedback() {
            // Given
            let mode: Mode<TestTransformation> = Mode {
                absolute_mode: AbsoluteMode::ToggleButtons,
                ..Default::default()
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
            assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
            assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(1.0));
        }

        #[test]
        fn feedback_target_interval() {
            // Given
            let mode: Mode<TestTransformation> = Mode {
                absolute_mode: AbsoluteMode::ToggleButtons,
                target_value_interval: create_unit_value_interval(0.3, 0.7),
                ..Default::default()
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
            assert_abs_diff_eq!(mode.feedback(con_val(0.4)).unwrap(), con_val(0.25));
            assert_abs_diff_eq!(mode.feedback(con_val(0.7)).unwrap(), con_val(1.0));
            assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(1.0));
        }
    }

    mod relative {
        use super::*;

        mod absolute_continuous_target {
            use super::*;

            #[test]
            fn default_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(rel(-10), &target, ()).is_none());
                assert!(mode.control(rel(-2), &target, ()).is_none());
                assert!(mode.control(rel(-1), &target, ()).is_none());
                assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.01));
                assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.01));
                assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.01));
            }

            #[test]
            fn default_2() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.99));
                assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.99));
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.99));
                assert!(mode.control(rel(1), &target, ()).is_none());
                assert!(mode.control(rel(2), &target, ()).is_none());
                assert!(mode.control(rel(10), &target, ()).is_none());
            }

            #[test]
            fn min_step_size_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(rel(-10), &target, ()).is_none());
                assert!(mode.control(rel(-2), &target, ()).is_none());
                assert!(mode.control(rel(-1), &target, ()).is_none());
                assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.4));
                assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(1.0));
            }

            #[test]
            fn min_step_size_2() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.0));
                assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.6));
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.8));
                assert!(mode.control(rel(1), &target, ()).is_none());
                assert!(mode.control(rel(2), &target, ()).is_none());
                assert!(mode.control(rel(10), &target, ()).is_none());
            }

            #[test]
            fn max_step_size_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(rel(-10), &target, ()).is_none());
                assert!(mode.control(rel(-2), &target, ()).is_none());
                assert!(mode.control(rel(-1), &target, ()).is_none());
                assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.01));
                assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.02));
                assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.09));
            }

            #[test]
            fn max_step_size_2() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.91));
                assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.98));
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.99));
                assert!(mode.control(rel(1), &target, ()).is_none());
                assert!(mode.control(rel(2), &target, ()).is_none());
                assert!(mode.control(rel(10), &target, ()).is_none());
            }

            #[test]
            fn reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.01));
                assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.01));
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.01));
                assert!(mode.control(rel(1), &target, ()).is_none());
                assert!(mode.control(rel(2), &target, ()).is_none());
                assert!(mode.control(rel(10), &target, ()).is_none());
            }

            #[test]
            fn rotate_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(1.0));
                assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(1.0));
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(1.0));
                assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.01));
                assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.01));
                assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.01));
            }

            #[test]
            fn rotate_2() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.99));
                assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.99));
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.99));
                assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.0));
                assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.0));
                assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.0));
            }

            #[test]
            fn target_interval_min() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.2)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(rel(-10), &target, ()).is_none());
                assert!(mode.control(rel(-2), &target, ()).is_none());
                assert!(mode.control(rel(-1), &target, ()).is_none());
                assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.21));
                assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.21));
                assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.21));
            }

            #[test]
            fn target_interval_max() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.8)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.79));
                assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.79));
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.79));
                assert!(mode.control(rel(1), &target, ()).is_none());
                assert!(mode.control(rel(2), &target, ()).is_none());
                assert!(mode.control(rel(10), &target, ()).is_none());
            }

            #[test]
            fn target_interval_current_target_value_out_of_range() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.2));
                assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.2));
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.2));
                assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
            }

            #[test]
            fn target_interval_current_target_value_just_appearing_out_of_range() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.199999999999)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.2));
                assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.2));
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.2));
                assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.21));
                assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.21));
                assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.21));
            }

            /// See https://github.com/helgoboss/realearn/issues/100.
            #[test]
            fn not_get_stuck() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    target_value_interval: full_unit_interval(),
                    step_size_interval: create_unit_value_interval(0.01, 0.01),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.875)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.865));
            }

            #[test]
            fn target_interval_min_rotate() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.2)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.8));
                assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.8));
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.8));
                assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.21));
                assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.21));
                assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.21));
            }

            #[test]
            fn target_interval_max_rotate() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.8)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.79));
                assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.79));
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.79));
                assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
            }

            #[test]
            fn target_interval_rotate_current_target_value_out_of_range() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.8));
                assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.8));
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.8));
                assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
            }

            #[test]
            fn make_absolute_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    convert_relative_to_absolute: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert_eq!(mode.control(rel(-10), &target, ()), None);
                assert_eq!(mode.control(rel(-2), &target, ()), None);
                assert_eq!(mode.control(rel(-1), &target, ()), None);
                assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.01));
                assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.02));
                assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.03));
                assert_abs_diff_eq!(mode.control(rel(-5), &target, ()).unwrap(), abs_con(0.02));
            }

            #[test]
            fn make_absolute_2() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    convert_relative_to_absolute: true,
                    step_size_interval: create_unit_value_interval(0.01, 0.05),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.5)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                // TODO-medium This behavior is debatable! Normal absolute control elements don't
                //  send the same absolute value multiple times when hitting knob/fader boundary.
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.0));
                assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.0));
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.0));
                assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.01));
                assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.03));
                assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.08));
                assert_abs_diff_eq!(mode.control(rel(-5), &target, ()).unwrap(), abs_con(0.03));
            }
        }

        mod absolute_discrete_target {
            use super::*;

            mod continuous_processing {
                use super::*;

                #[test]
                fn default_1() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert!(mode.control(rel(-10), &target, ()).is_none());
                    assert!(mode.control(rel(-2), &target, ()).is_none());
                    assert!(mode.control(rel(-1), &target, ()).is_none());
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.05));
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.05));
                    assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.05));
                }

                #[test]
                fn default_2() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(20, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel(-10), &target, ()).unwrap(),
                        abs_con(0.95)
                    );
                    assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.95));
                    assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.95));
                    assert!(mode.control(rel(1), &target, ()).is_none());
                    assert!(mode.control(rel(2), &target, ()).is_none());
                    assert!(mode.control(rel(10), &target, ()).is_none());
                }

                #[test]
                fn min_step_count_1() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        step_count_interval: create_discrete_increment_interval(4, 100),
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert!(mode.control(rel(-10), &target, ()).is_none());
                    assert!(mode.control(rel(-2), &target, ()).is_none());
                    assert!(mode.control(rel(-1), &target, ()).is_none());
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.20));
                    // 4x
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.25));
                    // 5x
                    assert_abs_diff_eq!(mode.control(rel(4), &target, ()).unwrap(), abs_con(0.35));
                    // 7x
                    assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.65));
                    // 13x
                    assert_abs_diff_eq!(
                        mode.control(rel(100), &target, ()).unwrap(),
                        abs_con(1.00)
                    );
                    // 100x
                }

                #[test]
                fn min_step_count_2() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        step_count_interval: create_discrete_increment_interval(4, 100),
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(20, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel(-10), &target, ()).unwrap(),
                        abs_con(0.35)
                    );
                    // 13x
                    assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.75));
                    // 5x
                    assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.8)); // 4x
                    assert!(mode.control(rel(1), &target, ()).is_none());
                    assert!(mode.control(rel(2), &target, ()).is_none());
                    assert!(mode.control(rel(10), &target, ()).is_none());
                }

                #[test]
                fn max_step_count_1() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        step_count_interval: create_discrete_increment_interval(1, 2),
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert!(mode.control(rel(-10), &target, ()).is_none());
                    assert!(mode.control(rel(-2), &target, ()).is_none());
                    assert!(mode.control(rel(-1), &target, ()).is_none());
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.05));
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.10));
                    assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.10));
                }

                #[test]
                fn max_step_count_throttle() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        step_count_interval: create_discrete_increment_interval(-2, -2),
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    // No effect because already min
                    assert!(mode.control(rel(-10), &target, ()).is_none());
                    assert!(mode.control(rel(-10), &target, ()).is_none());
                    assert!(mode.control(rel(-10), &target, ()).is_none());
                    assert!(mode.control(rel(-10), &target, ()).is_none());
                    // Every 2nd time
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.05));
                    assert!(mode.control(rel(1), &target, ()).is_none());
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.05));
                    assert!(mode.control(rel(2), &target, ()).is_none());
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.05));
                }

                #[test]
                fn max_step_count_2() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        step_count_interval: create_discrete_increment_interval(1, 2),
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(20, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel(-10), &target, ()).unwrap(),
                        abs_con(0.90)
                    );
                    assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.90));
                    assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.95));
                    assert!(mode.control(rel(1), &target, ()).is_none());
                    assert!(mode.control(rel(2), &target, ()).is_none());
                    assert!(mode.control(rel(10), &target, ()).is_none());
                }

                #[test]
                fn reverse() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        reverse: true,
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel(-10), &target, ()).unwrap(),
                        abs_con(0.05)
                    );
                    assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.05));
                    assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.05));
                    assert!(mode.control(rel(1), &target, ()).is_none());
                    assert!(mode.control(rel(2), &target, ()).is_none());
                    assert!(mode.control(rel(10), &target, ()).is_none());
                }

                #[test]
                fn rotate_1() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        rotate: true,
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(1.0));
                    assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(1.0));
                    assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(1.0));
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.05));
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.05));
                    assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.05));
                }

                #[test]
                fn rotate_2() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        rotate: true,
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(20, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel(-10), &target, ()).unwrap(),
                        abs_con(0.95)
                    );
                    assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.95));
                    assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.95));
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.0));
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.0));
                    assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.0));
                }

                #[test]
                fn target_interval_min() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        target_value_interval: create_unit_value_interval(0.2, 0.8),
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(4, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert!(mode.control(rel(-10), &target, ()).is_none());
                    assert!(mode.control(rel(-2), &target, ()).is_none());
                    assert!(mode.control(rel(-1), &target, ()).is_none());
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.25));
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.25));
                    assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.25));
                }

                #[test]
                fn target_interval_max() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        target_value_interval: create_unit_value_interval(0.2, 0.8),
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(16, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel(-10), &target, ()).unwrap(),
                        abs_con(0.75)
                    );
                    assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.75));
                    assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.75));
                    assert!(mode.control(rel(1), &target, ()).is_none());
                    assert!(mode.control(rel(2), &target, ()).is_none());
                    assert!(mode.control(rel(10), &target, ()).is_none());
                }

                #[test]
                fn target_interval_current_target_value_out_of_range() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        target_value_interval: create_unit_value_interval(0.2, 0.8),
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.2));
                    assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.2));
                    assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.2));
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                    assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
                }

                #[test]
                fn target_interval_step_interval_current_target_value_out_of_range() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        step_count_interval: create_discrete_increment_interval(1, 100),
                        target_value_interval: create_unit_value_interval(0.2, 0.8),
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.2));
                    assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.2));
                    assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.2));
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                    assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
                }

                #[test]
                fn target_interval_min_rotate() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        target_value_interval: create_unit_value_interval(0.2, 0.8),
                        rotate: true,
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(4, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.8));
                    assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.8));
                    assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.8));
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.25));
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.25));
                    assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.25));
                }

                #[test]
                fn target_interval_max_rotate() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        target_value_interval: create_unit_value_interval(0.2, 0.8),
                        rotate: true,
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(16, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel(-10), &target, ()).unwrap(),
                        abs_con(0.75)
                    );
                    assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.75));
                    assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.75));
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                    assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
                }

                #[test]
                fn target_interval_rotate_current_target_value_out_of_range() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        target_value_interval: create_unit_value_interval(0.2, 0.8),
                        rotate: true,
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.8));
                    assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.8));
                    assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.8));
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                    assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
                }

                #[test]
                fn make_absolute_1() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        convert_relative_to_absolute: true,
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert_eq!(mode.control(rel(-10), &target, ()), None);
                    assert_eq!(mode.control(rel(-2), &target, ()), None);
                    assert_eq!(mode.control(rel(-1), &target, ()), None);
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.01));
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.02));
                    assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.03));
                    assert_abs_diff_eq!(mode.control(rel(-5), &target, ()).unwrap(), abs_con(0.02));
                }

                #[test]
                fn make_absolute_2() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        convert_relative_to_absolute: true,
                        step_size_interval: create_unit_value_interval(0.01, 0.05),
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(10, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(0.05),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.0));
                    assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.0));
                    assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.0));
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.01));
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.03));
                    assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.08));
                    assert_abs_diff_eq!(mode.control(rel(-5), &target, ()).unwrap(), abs_con(0.03));
                }
            }

            mod discrete_processing {
                use super::*;

                #[test]
                fn default_1() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        use_discrete_processing: true,
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(1.0 / 20.0),
                        },
                    };
                    // When
                    // Then
                    assert_eq!(mode.control(rel(-10), &target, ()), None);
                    assert_eq!(mode.control(rel(-2), &target, ()), None);
                    assert_eq!(mode.control(rel(-1), &target, ()), None);
                    assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_dis(1, 20));
                    assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_dis(1, 20));
                    assert_abs_diff_eq!(
                        mode.control(rel(10), &target, ()).unwrap(),
                        abs_dis(1, 20)
                    );
                }

                #[test]
                fn default_2() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        use_discrete_processing: true,
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(20, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(1.0 / 20.0),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel(-10), &target, ()).unwrap(),
                        abs_dis(19, 20)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel(-2), &target, ()).unwrap(),
                        abs_dis(19, 20)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel(-1), &target, ()).unwrap(),
                        abs_dis(19, 20)
                    );
                    assert_eq!(mode.control(rel(1), &target, ()), None);
                    assert_eq!(mode.control(rel(2), &target, ()), None);
                    assert_eq!(mode.control(rel(10), &target, ()), None);
                }

                #[test]
                fn min_step_count_1() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        use_discrete_processing: true,
                        step_count_interval: create_discrete_increment_interval(4, 100),
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(0, 200)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(1.0 / 200.0),
                        },
                    };
                    // When
                    // Then
                    assert!(mode.control(rel(-10), &target, ()).is_none());
                    assert!(mode.control(rel(-2), &target, ()).is_none());
                    assert!(mode.control(rel(-1), &target, ()).is_none());
                    assert_abs_diff_eq!(
                        mode.control(rel(1), &target, ()).unwrap(),
                        abs_dis(4, 200)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel(2), &target, ()).unwrap(),
                        abs_dis(5, 200)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel(4), &target, ()).unwrap(),
                        abs_dis(7, 200)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel(10), &target, ()).unwrap(),
                        abs_dis(13, 200)
                    );
                    assert_abs_diff_eq!(
                        mode.control(rel(100), &target, ()).unwrap(),
                        abs_dis(100, 200)
                    );
                }

                #[test]
                fn min_step_count_2() {
                    // Given
                    let mut mode: Mode<TestTransformation> = Mode {
                        step_count_interval: create_discrete_increment_interval(4, 100),
                        ..Default::default()
                    };
                    let target = TestTarget {
                        current_value: Some(dis_val(20, 20)),
                        control_type: ControlType::AbsoluteDiscrete {
                            atomic_step_size: UnitValue::new(1.0 / 20.0),
                        },
                    };
                    // When
                    // Then
                    assert_abs_diff_eq!(
                        mode.control(rel(-10), &target, ()).unwrap(),
                        abs_con(0.35)
                    );
                    // 13x
                    assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.75));
                    // 5x
                    assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.8)); // 4x
                    assert!(mode.control(rel(1), &target, ()).is_none());
                    assert!(mode.control(rel(2), &target, ()).is_none());
                    assert!(mode.control(rel(10), &target, ()).is_none());
                }
                //
                // #[test]
                // fn max_step_count_1() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         step_count_interval: create_discrete_increment_interval(1, 2),
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert!(mode.control(rel(-10), &target, ()).is_none());
                //     assert!(mode.control(rel(-2), &target, ()).is_none());
                //     assert!(mode.control(rel(-1), &target, ()).is_none());
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.05));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.10));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.10));
                // }
                //
                // #[test]
                // fn max_step_count_throttle() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         step_count_interval: create_discrete_increment_interval(-2, -2),
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     // No effect because already min
                //     assert!(mode.control(rel(-10), &target, ()).is_none());
                //     assert!(mode.control(rel(-10), &target, ()).is_none());
                //     assert!(mode.control(rel(-10), &target, ()).is_none());
                //     assert!(mode.control(rel(-10), &target, ()).is_none());
                //     // Every 2nd time
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.05));
                //     assert!(mode.control(rel(1), &target, ()).is_none());
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.05));
                //     assert!(mode.control(rel(2), &target, ()).is_none());
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.05));
                // }
                //
                // #[test]
                // fn max_step_count_2() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         step_count_interval: create_discrete_increment_interval(1, 2),
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(20, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(
                //         mode.control(rel(-10), &target, ()).unwrap(),
                //         abs_con(0.90)
                //     );
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.90));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.95));
                //     assert!(mode.control(rel(1), &target, ()).is_none());
                //     assert!(mode.control(rel(2), &target, ()).is_none());
                //     assert!(mode.control(rel(10), &target, ()).is_none());
                // }
                //
                // #[test]
                // fn reverse() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         reverse: true,
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(
                //         mode.control(rel(-10), &target, ()).unwrap(),
                //         abs_con(0.05)
                //     );
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.05));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.05));
                //     assert!(mode.control(rel(1), &target, ()).is_none());
                //     assert!(mode.control(rel(2), &target, ()).is_none());
                //     assert!(mode.control(rel(10), &target, ()).is_none());
                // }
                //
                // #[test]
                // fn rotate_1() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         rotate: true,
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(1.0));
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(1.0));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(1.0));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.05));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.05));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.05));
                // }
                //
                // #[test]
                // fn rotate_2() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         rotate: true,
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(20, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(
                //         mode.control(rel(-10), &target, ()).unwrap(),
                //         abs_con(0.95)
                //     );
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.95));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.95));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.0));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.0));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.0));
                // }
                //
                // #[test]
                // fn target_interval_min() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         target_value_interval: create_unit_value_interval(0.2, 0.8),
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(4, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert!(mode.control(rel(-10), &target, ()).is_none());
                //     assert!(mode.control(rel(-2), &target, ()).is_none());
                //     assert!(mode.control(rel(-1), &target, ()).is_none());
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.25));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.25));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.25));
                // }
                //
                // #[test]
                // fn target_interval_max() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         target_value_interval: create_unit_value_interval(0.2, 0.8),
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(16, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(
                //         mode.control(rel(-10), &target, ()).unwrap(),
                //         abs_con(0.75)
                //     );
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.75));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.75));
                //     assert!(mode.control(rel(1), &target, ()).is_none());
                //     assert!(mode.control(rel(2), &target, ()).is_none());
                //     assert!(mode.control(rel(10), &target, ()).is_none());
                // }
                //
                // #[test]
                // fn target_interval_current_target_value_out_of_range() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         target_value_interval: create_unit_value_interval(0.2, 0.8),
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
                // }
                //
                // #[test]
                // fn target_interval_step_interval_current_target_value_out_of_range() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         step_count_interval: create_discrete_increment_interval(1, 100),
                //         target_value_interval: create_unit_value_interval(0.2, 0.8),
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
                // }
                //
                // #[test]
                // fn target_interval_min_rotate() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         target_value_interval: create_unit_value_interval(0.2, 0.8),
                //         rotate: true,
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(4, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.8));
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.8));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.8));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.25));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.25));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.25));
                // }
                //
                // #[test]
                // fn target_interval_max_rotate() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         target_value_interval: create_unit_value_interval(0.2, 0.8),
                //         rotate: true,
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(16, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(
                //         mode.control(rel(-10), &target, ()).unwrap(),
                //         abs_con(0.75)
                //     );
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.75));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.75));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
                // }
                //
                // #[test]
                // fn target_interval_rotate_current_target_value_out_of_range() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         target_value_interval: create_unit_value_interval(0.2, 0.8),
                //         rotate: true,
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.8));
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.8));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.8));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.2));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.2));
                // }

                // #[test]
                // fn make_absolute_1() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         convert_relative_to_absolute: true,
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(0, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_eq!(mode.control(rel(-10), &target, ()), None);
                //     assert_eq!(mode.control(rel(-2), &target, ()), None);
                //     assert_eq!(mode.control(rel(-1), &target, ()), None);
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.01));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.02));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.03));
                //     assert_abs_diff_eq!(mode.control(rel(-5), &target, ()).unwrap(), abs_con(0.02));
                // }
                //
                // #[test]
                // fn make_absolute_2() {
                //     // Given
                //     let mut mode: Mode<TestTransformation> = Mode {
                //         convert_relative_to_absolute: true,
                //         step_size_interval: create_unit_value_interval(0.01, 0.05),
                //         ..Default::default()
                //     };
                //     let target = TestTarget {
                //         current_value: Some(dis_val(10, 20)),
                //         control_type: ControlType::AbsoluteDiscrete {
                //             atomic_step_size: UnitValue::new(1.0 / 21.0),
                //         },
                //     };
                //     // When
                //     // Then
                //     assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.0));
                //     assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.0));
                //     assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.0));
                //     assert_abs_diff_eq!(mode.control(rel(1), &target, ()).unwrap(), abs_con(0.01));
                //     assert_abs_diff_eq!(mode.control(rel(2), &target, ()).unwrap(), abs_con(0.03));
                //     assert_abs_diff_eq!(mode.control(rel(10), &target, ()).unwrap(), abs_con(0.08));
                //     assert_abs_diff_eq!(mode.control(rel(-5), &target, ()).unwrap(), abs_con(0.03));
                // }
            }
        }

        mod relative_target {
            use super::*;

            #[test]
            fn default() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_eq!(mode.control(rel(-10), &target, ()), Some(rel(-1)));
                assert_eq!(mode.control(rel(-2), &target, ()), Some(rel(-1)));
                assert_eq!(mode.control(rel(-1), &target, ()), Some(rel(-1)));
                assert_eq!(mode.control(rel(1), &target, ()), Some(rel(1)));
                assert_eq!(mode.control(rel(2), &target, ()), Some(rel(1)));
                assert_eq!(mode.control(rel(10), &target, ()), Some(rel(1)));
            }

            #[test]
            fn min_step_count() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    step_count_interval: create_discrete_increment_interval(2, 100),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_eq!(mode.control(rel(-10), &target, ()), Some(rel(-11)));
                assert_eq!(mode.control(rel(-2), &target, ()), Some(rel(-3)));
                assert_eq!(mode.control(rel(-1), &target, ()), Some(rel(-2)));
                assert_eq!(mode.control(rel(1), &target, ()), Some(rel(2)));
                assert_eq!(mode.control(rel(2), &target, ()), Some(rel(3)));
                assert_eq!(mode.control(rel(10), &target, ()), Some(rel(11)));
            }

            #[test]
            fn min_step_count_throttle() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    step_count_interval: create_discrete_increment_interval(-4, 100),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                // So intense that reaching speedup area
                assert_eq!(mode.control(rel(-10), &target, ()), Some(rel(-6)));
                // Every 3rd time
                assert_eq!(mode.control(rel(-2), &target, ()), Some(rel(-1)));
                assert_eq!(mode.control(rel(-2), &target, ()), None);
                assert_eq!(mode.control(rel(-2), &target, ()), None);
                assert_eq!(mode.control(rel(-2), &target, ()), Some(rel(-1)));
                // Every 4th time (but fired before)
                assert_eq!(mode.control(rel(-1), &target, ()), None);
                assert_eq!(mode.control(rel(-1), &target, ()), None);
                assert_eq!(mode.control(rel(-1), &target, ()), None);
                assert_eq!(mode.control(rel(-1), &target, ()), Some(rel(-1)));
                // Direction change
                assert_eq!(mode.control(rel(1), &target, ()), None);
                // Every 3rd time (but fired before)
                assert_eq!(mode.control(rel(2), &target, ()), Some(rel(1)));
                assert_eq!(mode.control(rel(2), &target, ()), None);
                assert_eq!(mode.control(rel(2), &target, ()), None);
                assert_eq!(mode.control(rel(2), &target, ()), Some(rel(1)));
                // So intense that reaching speedup area
                assert_eq!(mode.control(rel(10), &target, ()), Some(rel(6)));
            }

            #[test]
            fn max_step_count() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    step_count_interval: create_discrete_increment_interval(1, 2),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_eq!(mode.control(rel(-10), &target, ()), Some(rel(-2)));
                assert_eq!(mode.control(rel(-2), &target, ()), Some(rel(-2)));
                assert_eq!(mode.control(rel(-1), &target, ()), Some(rel(-1)));
                assert_eq!(mode.control(rel(1), &target, ()), Some(rel(1)));
                assert_eq!(mode.control(rel(2), &target, ()), Some(rel(2)));
                assert_eq!(mode.control(rel(10), &target, ()), Some(rel(2)));
            }

            #[test]
            fn max_step_count_throttle() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    step_count_interval: create_discrete_increment_interval(-10, -4),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                // Every 4th time
                assert_eq!(mode.control(rel(-10), &target, ()), Some(rel(-1)));
                assert_eq!(mode.control(rel(-10), &target, ()), None);
                assert_eq!(mode.control(rel(-10), &target, ()), None);
                assert_eq!(mode.control(rel(-10), &target, ()), None);
                assert_eq!(mode.control(rel(-10), &target, ()), Some(rel(-1)));
                assert_eq!(mode.control(rel(-10), &target, ()), None);
                assert_eq!(mode.control(rel(-10), &target, ()), None);
                assert_eq!(mode.control(rel(-10), &target, ()), None);
                // Every 10th time
                assert_eq!(mode.control(rel(1), &target, ()), None);
                assert_eq!(mode.control(rel(1), &target, ()), None);
                assert_eq!(mode.control(rel(1), &target, ()), None);
                assert_eq!(mode.control(rel(1), &target, ()), None);
                assert_eq!(mode.control(rel(1), &target, ()), Some(rel(1)));
                assert_eq!(mode.control(rel(1), &target, ()), None);
                assert_eq!(mode.control(rel(1), &target, ()), None);
                assert_eq!(mode.control(rel(1), &target, ()), None);
                assert_eq!(mode.control(rel(1), &target, ()), None);
                assert_eq!(mode.control(rel(1), &target, ()), None);
                assert_eq!(mode.control(rel(1), &target, ()), None);
                assert_eq!(mode.control(rel(1), &target, ()), None);
                assert_eq!(mode.control(rel(1), &target, ()), None);
                assert_eq!(mode.control(rel(1), &target, ()), None);
                assert_eq!(mode.control(rel(1), &target, ()), Some(rel(1)));
            }

            #[test]
            fn reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_eq!(mode.control(rel(-10), &target, ()), Some(rel(1)));
                assert_eq!(mode.control(rel(-2), &target, ()), Some(rel(1)));
                assert_eq!(mode.control(rel(-1), &target, ()), Some(rel(1)));
                assert_eq!(mode.control(rel(1), &target, ()), Some(rel(-1)));
                assert_eq!(mode.control(rel(2), &target, ()), Some(rel(-1)));
                assert_eq!(mode.control(rel(10), &target, ()), Some(rel(-1)));
            }
        }
    }

    mod incremental_buttons {
        use super::*;

        mod absolute_continuous_target {
            use super::*;

            #[test]
            fn default_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.01)
                );
            }

            #[test]
            fn default_2() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con(1.0), &target, ()).is_none());
            }

            #[test]
            fn min_step_size_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.28)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.6)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
            }

            #[test]
            fn min_step_size_2() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con(1.0), &target, ()).is_none());
            }

            #[test]
            fn max_step_size_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.018)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.05)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.75), &target, ()).unwrap(),
                    abs_con(0.07)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.09)
                );
            }

            #[test]
            fn max_step_size_2() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con(1.0), &target, ()).is_none());
            }

            #[test]
            fn source_interval() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.25), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.75), &target, ()).unwrap(),
                    abs_con(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.01)
                );
            }

            #[test]
            fn source_interval_step_interval() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    step_size_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.25), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.5)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.75), &target, ()).unwrap(),
                    abs_con(0.75)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
            }

            #[test]
            fn reverse_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con(1.0), &target, ()).is_none());
            }

            #[test]
            fn reverse_2() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.99)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.99)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.99)
                );
            }

            #[test]
            fn rotate_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.01)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.01)
                );
            }

            #[test]
            fn rotate_2() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
            }

            #[test]
            fn rotate_3_almost_max() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.990000000001)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
            }

            #[test]
            fn reverse_and_rotate_almost_min() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    rotate: true,
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.00999999999999)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
            }

            #[test]
            fn reverse_and_rotate_min() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    rotate: true,
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
            }

            #[test]
            fn target_interval_min() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.2)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.21)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.21)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.21)
                );
            }

            #[test]
            fn target_interval_max() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.8)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.1), &target, ()).is_none());
                assert!(mode.control(abs_con(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con(1.0), &target, ()).is_none());
            }

            #[test]
            fn target_interval_current_target_value_out_of_range() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.2)
                );
            }

            #[test]
            fn target_interval_min_rotate() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.2)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.21)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.21)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.21)
                );
            }

            #[test]
            fn target_interval_max_rotate() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.8)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.2)
                );
            }

            #[test]
            fn target_interval_rotate_current_target_value_out_of_range() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.2)
                );
            }

            #[test]
            fn target_interval_rotate_reverse_current_target_value_out_of_range() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    reverse: true,
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.8)
                );
            }
        }

        mod absolute_discrete_target {
            use super::*;

            #[test]
            fn default_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.05)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.05)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.05)
                );
            }

            #[test]
            fn default_2() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.1), &target, ()).is_none());
                assert!(mode.control(abs_con(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con(1.0), &target, ()).is_none());
            }

            #[test]
            fn min_step_count_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    step_count_interval: create_discrete_increment_interval(4, 8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.3)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.4)
                );
            }

            #[test]
            fn min_step_count_throttle() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    step_count_interval: create_discrete_increment_interval(-4, -4),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                // Every 4th time
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.05)
                );
                assert!(mode.control(abs_con(0.1), &target, ()).is_none());
                assert!(mode.control(abs_con(0.1), &target, ()).is_none());
                assert!(mode.control(abs_con(0.1), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.05)
                );
            }

            #[test]
            fn min_step_count_2() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    step_count_interval: create_discrete_increment_interval(4, 8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.1), &target, ()).is_none());
                assert!(mode.control(abs_con(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con(1.0), &target, ()).is_none());
            }

            #[test]
            fn max_step_count_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    step_count_interval: create_discrete_increment_interval(1, 8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.1)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.25)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.4)
                );
            }

            #[test]
            fn max_step_count_2() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    step_count_interval: create_discrete_increment_interval(1, 2),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.90));
                assert_abs_diff_eq!(mode.control(rel(-2), &target, ()).unwrap(), abs_con(0.90));
                assert_abs_diff_eq!(mode.control(rel(-1), &target, ()).unwrap(), abs_con(0.95));
                assert!(mode.control(rel(1), &target, ()).is_none());
                assert!(mode.control(rel(2), &target, ()).is_none());
                assert!(mode.control(rel(10), &target, ()).is_none());
            }

            #[test]
            fn source_interval() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.25), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.05)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.75), &target, ()).unwrap(),
                    abs_con(0.05)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.05)
                );
            }

            #[test]
            fn source_interval_step_interval() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    step_count_interval: create_discrete_increment_interval(4, 8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.25), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.75), &target, ()).unwrap(),
                    abs_con(0.3)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.4)
                );
            }

            #[test]
            fn reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.1), &target, ()).is_none());
                assert!(mode.control(abs_con(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con(1.0), &target, ()).is_none());
            }

            #[test]
            fn rotate_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.05)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.05)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.05)
                );
            }

            #[test]
            fn rotate_2() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(1.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.0)
                );
            }

            #[test]
            fn target_interval_min() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.2)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.25)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.25)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.25)
                );
            }

            #[test]
            fn target_interval_max() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.8)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.1), &target, ()).is_none());
                assert!(mode.control(abs_con(0.5), &target, ()).is_none());
                assert!(mode.control(abs_con(1.0), &target, ()).is_none());
            }

            #[test]
            fn target_interval_current_target_value_out_of_range() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.2)
                );
            }

            #[test]
            fn step_count_interval_exceeded() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    step_count_interval: create_discrete_increment_interval(1, 100),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.55)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(1.0)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(1.0)
                );
            }

            #[test]
            fn target_interval_step_interval_current_target_value_out_of_range() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    step_count_interval: create_discrete_increment_interval(1, 100),
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.2)
                );
            }

            #[test]
            fn target_interval_min_rotate() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.2)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.25)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.25)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.25)
                );
            }

            #[test]
            fn target_interval_max_rotate() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.8)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.2)
                );
            }

            #[test]
            fn target_interval_rotate_current_target_value_out_of_range() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.2)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.2)
                );
            }

            #[test]
            fn target_interval_rotate_reverse_current_target_value_out_of_range() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    reverse: true,
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.1), &target, ()).unwrap(),
                    abs_con(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(0.5), &target, ()).unwrap(),
                    abs_con(0.8)
                );
                assert_abs_diff_eq!(
                    mode.control(abs_con(1.0), &target, ()).unwrap(),
                    abs_con(0.8)
                );
            }
        }

        mod relative_target {
            use super::*;

            #[test]
            fn default() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(mode.control(abs_con(0.1), &target, ()).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.control(abs_con(0.5), &target, ()).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.control(abs_con(1.0), &target, ()).unwrap(), rel(1));
            }

            #[test]
            fn min_step_count() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    step_count_interval: create_discrete_increment_interval(2, 8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(mode.control(abs_con(0.1), &target, ()).unwrap(), rel(3));
                assert_abs_diff_eq!(mode.control(abs_con(0.5), &target, ()).unwrap(), rel(5));
                assert_abs_diff_eq!(mode.control(abs_con(1.0), &target, ()).unwrap(), rel(8));
            }

            #[test]
            fn max_step_count() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    step_count_interval: create_discrete_increment_interval(1, 2),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(mode.control(abs_con(0.1), &target, ()).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.control(abs_con(0.5), &target, ()).unwrap(), rel(2));
                assert_abs_diff_eq!(mode.control(abs_con(1.0), &target, ()).unwrap(), rel(2));
            }

            #[test]
            fn source_interval() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.25), &target, ()).is_none());
                assert_abs_diff_eq!(mode.control(abs_con(0.5), &target, ()).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.control(abs_con(1.0), &target, ()).unwrap(), rel(1));
            }

            #[test]
            fn source_interval_step_interval() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    step_count_interval: create_discrete_increment_interval(4, 8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert!(mode.control(abs_con(0.25), &target, ()).is_none());
                assert_abs_diff_eq!(mode.control(abs_con(0.5), &target, ()).unwrap(), rel(4));
                assert_abs_diff_eq!(mode.control(abs_con(1.0), &target, ()).unwrap(), rel(8));
            }

            #[test]
            fn reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(con_val(0.0)),
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert!(mode.control(abs_con(0.0), &target, ()).is_none());
                assert_abs_diff_eq!(mode.control(abs_con(0.1), &target, ()).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.control(abs_con(0.5), &target, ()).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.control(abs_con(1.0), &target, ()).unwrap(), rel(-1));
            }
        }

        mod feedback {
            use super::*;

            #[test]
            fn default() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(1.0));
            }

            #[test]
            fn reverse() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    reverse: true,
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(1.0));
                assert_abs_diff_eq!(mode.feedback(con_val(0.5)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.0));
            }

            #[test]
            fn source_and_target_interval() {
                // Given
                let mode: Mode<TestTransformation> = Mode {
                    absolute_mode: AbsoluteMode::IncrementalButtons,
                    source_value_interval: create_unit_value_interval(0.2, 0.8),
                    target_value_interval: create_unit_value_interval(0.4, 1.0),
                    ..Default::default()
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.feedback(con_val(0.0)).unwrap(), con_val(0.2));
                assert_abs_diff_eq!(mode.feedback(con_val(0.4)).unwrap(), con_val(0.2));
                assert_abs_diff_eq!(mode.feedback(con_val(0.7)).unwrap(), con_val(0.5));
                assert_abs_diff_eq!(mode.feedback(con_val(1.0)).unwrap(), con_val(0.8));
            }
        }
    }

    fn abs_con(number: f64) -> ControlValue {
        ControlValue::absolute_continuous(number)
    }

    fn abs_dis(actual: u32, max: u32) -> ControlValue {
        ControlValue::absolute_discrete(actual, max)
    }

    fn rel(increment: i32) -> ControlValue {
        ControlValue::relative(increment)
    }

    fn con_val(v: f64) -> AbsoluteValue {
        AbsoluteValue::Continuous(UnitValue::new(v))
    }

    fn dis_val(actual: u32, max: u32) -> AbsoluteValue {
        AbsoluteValue::Discrete(Fraction::new(actual, max))
    }

    const FB_OPTS: ModeFeedbackOptions = ModeFeedbackOptions {
        source_is_virtual: false,
        // Count: 101
        max_discrete_source_value: Some(100),
    };
}

pub fn default_step_size_interval() -> Interval<UnitValue> {
    // 0.01 has been chosen as default minimum step size because it corresponds to 1%.
    //
    // 0.01 has also been chosen as default maximum step size because most users probably
    // want to start easy, that is without using the "press harder = more increments"
    // respectively "dial harder = more increments" features. Activating them right from
    // the start by choosing a higher step size maximum could lead to surprising results
    // such as ugly parameters jumps, especially if the source is not suited for that.
    create_unit_value_interval(0.01, 0.01)
}

pub fn default_step_count_interval() -> Interval<DiscreteIncrement> {
    // Same reasoning as with step size interval
    create_discrete_increment_interval(1, 1)
}

pub enum ModeControlResult<T> {
    /// Target should be hit with the given value.
    HitTarget(T),
    /// Target is reached but already has the given desired value and is not retriggerable.
    /// It shouldn't be hit.
    LeaveTargetUntouched(T),
}

impl<T> ModeControlResult<T> {
    pub fn map<R>(self, f: impl FnOnce(T) -> R) -> ModeControlResult<R> {
        use ModeControlResult::*;
        match self {
            HitTarget(v) => HitTarget(f(v)),
            LeaveTargetUntouched(v) => LeaveTargetUntouched(f(v)),
        }
    }
}

impl<T> From<ModeControlResult<T>> for Option<T> {
    fn from(res: ModeControlResult<T>) -> Self {
        use ModeControlResult::*;
        match res {
            LeaveTargetUntouched(_) => None,
            HitTarget(v) => Some(v),
        }
    }
}

fn full_discrete_interval() -> Interval<u32> {
    Interval::new(0, u32::MAX)
}
