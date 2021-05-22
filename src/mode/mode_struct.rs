use crate::{
    create_discrete_increment_interval, create_unit_value_interval, full_unit_interval,
    mode::feedback_util, negative_if, AbsoluteValue, ButtonUsage, ControlType, ControlValue,
    DiscreteIncrement, DiscreteValue, EncoderUsage, Interval, MinIsMaxBehavior, OutOfRangeBehavior,
    PressDurationProcessor, TakeoverMode, Target, Transformation, UnitIncrement, UnitValue,
    BASE_EPSILON,
};
use derive_more::Display;
use enum_iterator::IntoEnumIterator;
use num_enum::{IntoPrimitive, TryFromPrimitive};
#[cfg(feature = "serde_repr")]
use serde_repr::{Deserialize_repr, Serialize_repr};

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct ModeControlOptions {
    pub enforce_rotate: bool,
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
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, IntoEnumIterator, TryFromPrimitive, IntoPrimitive, Display,
)]
#[cfg_attr(feature = "serde_repr", derive(Serialize_repr, Deserialize_repr))]
#[repr(usize)]
pub enum AbsoluteMode {
    #[display(fmt = "Normal")]
    Normal = 0,
    #[display(fmt = "Discrete")]
    Discrete = 3,
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
        }
    }
}

impl<T: Transformation> Mode<T> {
    /// Processes the given control value and maybe returns an appropriate target control value.
    ///
    /// `None` either means ignored or target value already has desired value.
    pub fn control<'a, C: Copy>(
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

    /// Takes a target value, interprets and transforms it conforming to mode rules and
    /// maybe returns an appropriate source value that should be sent to the source.
    pub fn feedback(&self, target_value: UnitValue) -> Option<UnitValue> {
        feedback_util::feedback(
            target_value,
            self.reverse,
            &self.feedback_transformation,
            &self.source_value_interval,
            &self.target_value_interval,
            self.out_of_range_behavior,
        )
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
            Normal | Discrete => Some(
                self.control_absolute_normal_or_discrete(v, target, context)?
                    .map(ControlValue::from_absolute),
            ),
            IncrementalButtons => {
                self.control_absolute_incremental_buttons(v, target, context, options)
            }
            ToggleButtons => Some(
                self.control_absolute_toggle_buttons(v, target, context)?
                    .map(|v| ControlValue::AbsoluteContinuous(v.to_unit_value())),
            ),
        }
    }

    /// Processes the given control value in absolute mode and maybe returns an appropriate target
    /// value.
    fn control_absolute_normal_or_discrete<'a, C: Copy>(
        &mut self,
        control_value: AbsoluteValue,
        target: &impl Target<'a, Context = C>,
        context: C,
    ) -> Option<ModeControlResult<AbsoluteValue>> {
        // Memorize as previous value for next control cycle.
        let previous_control_value = self
            .previous_absolute_control_value
            .replace(control_value.to_unit_value());
        match self.button_usage {
            ButtonUsage::PressOnly if control_value.is_zero() => return None,
            ButtonUsage::ReleaseOnly if !control_value.is_zero() => return None,
            _ => {}
        };
        let (source_bound_value, min_is_max_behavior) = if control_value
            .is_within_interval_tolerant(
                &self.source_value_interval,
                &self.discrete_source_value_interval,
                BASE_EPSILON,
            ) {
            // Control value is within source value interval
            (control_value.to_unit_value(), MinIsMaxBehavior::PreferOne)
        } else {
            // Control value is outside source value interval
            use OutOfRangeBehavior::*;
            match self.out_of_range_behavior {
                MinOrMax => {
                    if control_value.to_unit_value() < self.source_value_interval.min_val() {
                        (
                            self.source_value_interval.min_val(),
                            MinIsMaxBehavior::PreferZero,
                        )
                    } else {
                        (
                            self.source_value_interval.max_val(),
                            MinIsMaxBehavior::PreferOne,
                        )
                    }
                }
                Min => (
                    self.source_value_interval.min_val(),
                    MinIsMaxBehavior::PreferZero,
                ),
                Ignore => return None,
            }
        };
        let current_target_value = target.current_value(context);
        // Control value is within source value interval
        let control_type = target.control_type();
        let pepped_up_control_value = self.pep_up_control_value(
            AbsoluteValue::Continuous(source_bound_value),
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
        control_value: AbsoluteValue,
        target: &impl Target<'a, Context = C>,
        context: C,
        options: ModeControlOptions,
    ) -> Option<ModeControlResult<ControlValue>> {
        if control_value.is_zero()
            || !control_value.is_within_interval_tolerant(
                &self.source_value_interval,
                &self.discrete_source_value_interval,
                BASE_EPSILON,
            )
        {
            return None;
        }
        use ControlType::*;
        match target.control_type() {
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
                    .map_to_unit_interval_from(
                        &self.source_value_interval,
                        MinIsMaxBehavior::PreferOne,
                        BASE_EPSILON
                    )
                    .map_from_unit_interval_to(&self.step_size_interval);
                let step_size_increment =
                    step_size_value.to_increment(negative_if(self.reverse))?;
                self.hit_target_absolutely_with_unit_increment(
                    step_size_increment,
                    self.step_size_interval.min_val(),
                    target.current_value(context)?,
                    options
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
                self.hit_discrete_target_absolutely(discrete_increment, atomic_step_size, options, || {
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
        Some(ModeControlResult::HitTarget(AbsoluteValue::Continuous(
            desired_target_value,
        )))
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
        self.control_absolute_normal_or_discrete(
            AbsoluteValue::Continuous(abs_input_value),
            target,
            context,
        )
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
        match target.control_type() {
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
                    target.current_value(context)?,
                    options
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
                self.hit_discrete_target_absolutely(pepped_up_increment, atomic_step_size, options, || {
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
        let mut v = control_value.map_to_unit_interval_from(
            &self.source_value_interval,
            min_is_max_behavior,
            BASE_EPSILON,
        );
        // 2. Apply transformation
        v = self
            .control_transformation
            .as_ref()
            .and_then(|t| {
                t.transform(v, current_target_value.unwrap_or_default().to_unit_value())
                    .ok()
            })
            .unwrap_or(v);
        // 3. Apply reverse
        v = if self.reverse { v.inverse() } else { v };
        // 4. Apply target interval
        v = v.map_from_unit_interval_to(&self.target_value_interval);
        // 5. Apply rounding
        v = if self.round_target_value {
            round_to_nearest_discrete_value(control_type, v)
        } else {
            v
        };
        AbsoluteValue::Continuous(v)
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
            None => return Some(ModeControlResult::HitTarget(control_value)),
            Some(v) => v,
        };
        if self.jump_interval.is_full() {
            // No jump restrictions whatsoever
            return self.hit_if_changed(control_value, current_target_value, control_type);
        }
        let distance = control_value.calc_distance_from(current_target_value.to_unit_value());
        if distance > self.jump_interval.max_val() {
            // Distance is too large
            use TakeoverMode::*;
            return match self.takeover_mode {
                Pickup => {
                    // Scaling not desired. Do nothing.
                    None
                }
                Parallel => {
                    if let Some(prev) = previous_control_value {
                        let relative_increment =
                            control_value.to_unit_value() - prev.to_unit_value();
                        if relative_increment == 0.0 {
                            None
                        } else {
                            let relative_increment = UnitIncrement::new(relative_increment);
                            let restrained_increment =
                                relative_increment.clamp_to_interval(&self.jump_interval)?;
                            let final_target_value = current_target_value.add_clamping(
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
                    let approach_distance = distance.map_from_unit_interval_to(&self.jump_interval);
                    let approach_increment = approach_distance.to_increment(negative_if(
                        control_value.to_unit_value() < current_target_value.to_unit_value(),
                    ))?;
                    let final_target_value = current_target_value.add_clamping(
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
                        let relative_increment =
                            control_value.to_unit_value() - prev.to_unit_value();
                        if relative_increment == 0.0 {
                            None
                        } else {
                            let goes_up = relative_increment.is_sign_positive();
                            let source_distance_from_border = if goes_up {
                                1.0 - prev.get()
                            } else {
                                prev.get()
                            };
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
                                    current_target_value,
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
        if distance < self.jump_interval.min_val() {
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
        if !control_type.is_retriggerable() && current_target_value == desired_target_value {
            return Some(ModeControlResult::LeaveTargetUntouched(
                desired_target_value,
            ));
        }
        Some(ModeControlResult::HitTarget(desired_target_value))
    }

    fn hit_discrete_target_absolutely(
        &self,
        discrete_increment: DiscreteIncrement,
        target_step_size: UnitValue,
        options: ModeControlOptions,
        current_value: impl Fn() -> Option<AbsoluteValue>,
    ) -> Option<ModeControlResult<ControlValue>> {
        let unit_increment = discrete_increment.to_unit_increment(target_step_size)?;
        self.hit_target_absolutely_with_unit_increment(
            unit_increment,
            target_step_size,
            current_value()?,
            options,
        )
    }

    fn hit_target_absolutely_with_unit_increment(
        &self,
        increment: UnitIncrement,
        grid_interval_size: UnitValue,
        current_target_value: AbsoluteValue,
        options: ModeControlOptions,
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
            current_target_value.to_unit_value()
        } else {
            current_target_value.snap_to_grid_by_interval_size(grid_interval_size)
        };
        v = if options.enforce_rotate || self.rotate {
            v.add_rotating(increment, &snapped_target_value_interval, BASE_EPSILON)
        } else {
            v.add_clamping(increment, &snapped_target_value_interval, BASE_EPSILON)
        };
        if v == current_target_value.to_unit_value() {
            return Some(ModeControlResult::LeaveTargetUntouched(
                ControlValue::AbsoluteContinuous(v),
            ));
        }
        Some(ModeControlResult::HitTarget(
            ControlValue::AbsoluteContinuous(v),
        ))
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
        control_value: AbsoluteValue,
    ) -> Option<DiscreteIncrement> {
        let factor = control_value
            .map_to_unit_interval_from(
                &self.source_value_interval,
                MinIsMaxBehavior::PreferOne,
                BASE_EPSILON,
            )
            .map_from_unit_interval_to_discrete_increment(&self.step_count_interval);
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

fn round_to_nearest_discrete_value(
    control_type: ControlType,
    approximate_control_value: UnitValue,
) -> UnitValue {
    // round() is the right choice here vs. floor() because we don't want slight numerical
    // inaccuracies lead to surprising jumps
    use ControlType::*;
    let step_size = match control_type {
        AbsoluteContinuousRoundable { rounding_step_size } => rounding_step_size,
        AbsoluteDiscrete { atomic_step_size } => atomic_step_size,
        AbsoluteContinuousRetriggerable
        | AbsoluteContinuous
        | Relative
        | VirtualMulti
        | VirtualButton => {
            return approximate_control_value;
        }
    };
    approximate_control_value.snap_to_grid_by_interval_size(step_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::mode::test_util::{TestTarget, TestTransformation};
    use crate::{create_unit_value_interval, ControlType, Fraction};
    use approx::*;

    mod absolute_normal {
        use super::*;

        #[test]
        fn default() {
            // Given
            let mut mode: Mode<TestTransformation> = Mode {
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(continuous_value(0.777)),
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
                mode.control(abs_con(0.5), &target, ()).unwrap(),
                abs_con(0.5)
            );
            assert_abs_diff_eq!(
                mode.control(abs_dis(63, 127), &target, ()).unwrap(),
                abs_con(0.49606299212598426)
            );
            assert!(mode.control(abs_con(0.777), &target, ()).is_none());
            assert!(mode.control(abs_dis(777, 1000), &target, ()).is_none());
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
        fn default_target_is_trigger() {
            // Given
            let mut mode: Mode<TestTransformation> = Mode {
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(continuous_value(0.777)),
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
                current_value: Some(continuous_value(0.777)),
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
            assert!(mode.control(abs_con(0.777), &target, ()).is_none());
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
                current_value: Some(continuous_value(0.777)),
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
                current_value: Some(continuous_value(0.777)),
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
                current_value: Some(continuous_value(0.777)),
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
                current_value: Some(continuous_value(0.777)),
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
                current_value: Some(continuous_value(0.777)),
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
                current_value: Some(continuous_value(0.777)),
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
                current_value: Some(continuous_value(0.777)),
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
                current_value: Some(continuous_value(0.777)),
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
                current_value: Some(continuous_value(0.777)),
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
                current_value: Some(continuous_value(0.777)),
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
                current_value: Some(continuous_value(0.777)),
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
        fn round() {
            // Given
            let mut mode: Mode<TestTransformation> = Mode {
                round_target_value: true,
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(continuous_value(0.777)),
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
                current_value: Some(continuous_value(0.5)),
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
                current_value: Some(continuous_value(0.5)),
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
                current_value: Some(continuous_value(0.5)),
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
                control_transformation: Some(TestTransformation::new(|input| Ok(input.inverse()))),
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(continuous_value(0.777)),
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
                current_value: Some(continuous_value(0.777)),
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
            assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(0.5));
            assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(1.0));
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
            assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(1.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(0.5));
            assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(0.0));
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
            assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.2)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.4)).unwrap(), uv(0.25));
            assert_abs_diff_eq!(mode.feedback(uv(0.6)).unwrap(), uv(0.5));
            assert_abs_diff_eq!(mode.feedback(uv(0.8)).unwrap(), uv(0.75));
            assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(1.0));
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
            assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(1.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.2)).unwrap(), uv(1.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.4)).unwrap(), uv(0.75));
            assert_abs_diff_eq!(mode.feedback(uv(0.6)).unwrap(), uv(0.5));
            assert_abs_diff_eq!(mode.feedback(uv(0.8)).unwrap(), uv(0.25));
            assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(0.0));
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
            assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.2));
            assert_abs_diff_eq!(mode.feedback(uv(0.4)).unwrap(), uv(0.2));
            assert_abs_diff_eq!(mode.feedback(uv(0.7)).unwrap(), uv(0.5));
            assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(0.8));
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
            assert!(mode.feedback(uv(0.0)).is_none());
            assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(0.5));
            assert!(mode.feedback(uv(1.0)).is_none());
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
            assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.1)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(0.5));
            assert_abs_diff_eq!(mode.feedback(uv(0.9)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(0.0));
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
            assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.01)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.02)).unwrap(), uv(1.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.03)).unwrap(), uv(0.0));
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
            assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.01)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.03)).unwrap(), uv(1.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.04)).unwrap(), uv(0.0));
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
            assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.01)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.029999999329447746)).unwrap(), uv(1.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.0300000001)).unwrap(), uv(1.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.04)).unwrap(), uv(0.0));
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
            assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.1)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(1.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.9)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(0.0));
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
            assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.1)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(1.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.9)).unwrap(), uv(1.0));
            assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(1.0));
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
            assert!(mode.feedback(uv(0.0)).is_none());
            assert!(mode.feedback(uv(0.1)).is_none());
            assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(1.0));
            assert!(mode.feedback(uv(0.9)).is_none());
            assert!(mode.feedback(uv(1.0)).is_none());
        }

        #[test]
        fn feedback_transformation() {
            // Given
            let mode: Mode<TestTransformation> = Mode {
                feedback_transformation: Some(TestTransformation::new(|input| Ok(input.inverse()))),
                ..Default::default()
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(1.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(0.5));
            assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(0.0));
        }
    }

    mod absolute_discrete {
        use super::*;

        #[test]
        fn default() {
            // Given
            let mut mode: Mode<TestTransformation> = Mode {
                absolute_mode: AbsoluteMode::Discrete,
                ..Default::default()
            };
            let target = TestTarget {
                current_value: Some(continuous_value(0.777)),
                control_type: ControlType::AbsoluteContinuous,
            };
            // When
            // Then
            assert_abs_diff_eq!(
                mode.control(abs_dis(0, 127), &target, ()).unwrap(),
                abs_dis(0, 127)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(0.0), &target, ()).unwrap(),
                abs_con(0.0)
            );
            assert_abs_diff_eq!(
                mode.control(abs_dis(63, 127), &target, ()).unwrap(),
                abs_dis(63, 127)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(0.5), &target, ()).unwrap(),
                abs_con(0.5)
            );
            assert!(mode.control(abs_dis(777, 1000), &target, ()).is_none());
            assert!(mode.control(abs_con(0.777), &target, ()).is_none());
            assert_abs_diff_eq!(
                mode.control(abs_dis(127, 127), &target, ()).unwrap(),
                abs_dis(127, 127)
            );
            assert_abs_diff_eq!(
                mode.control(abs_con(1.0), &target, ()).unwrap(),
                abs_con(1.0)
            );
        }
        //
        //     #[test]
        //     fn default_target_is_trigger() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteContinuousRetriggerable,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.5), &target, ()).unwrap(),
        //             abs_con(0.5)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.777), &target, ()).unwrap(),
        //             abs_con(0.777)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //     }
        //
        //     #[test]
        //     fn relative_target() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::Relative,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.5), &target, ()).unwrap(),
        //             abs_con(0.5)
        //         );
        //         assert!(mode.control(abs_con(0.777), &target, ()).is_none());
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //     }
        //
        //     #[test]
        //     fn source_interval() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             source_value_interval: create_unit_value_interval(0.2, 0.6),
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_dis(0, 127), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.1), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_dis(13, 127), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.2), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_dis(25, 127), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.4), &target, ()).unwrap(),
        //             abs_con(0.5)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_dis(51, 127), &target, ()).unwrap(),
        //             abs_con(0.5039370078740157)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.6), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_dis(76, 127), &target, ()).unwrap(),
        //             abs_con(0.9960629921259844)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.8), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_dis(80, 127), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_dis(127, 127), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //     }
        //
        //     #[test]
        //     fn source_interval_out_of_range_ignore() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             source_value_interval: create_unit_value_interval(0.2, 0.6),
        //             out_of_range_behavior: OutOfRangeBehavior::Ignore,
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert!(mode.control(abs_con(0.0), &target, ()).is_none());
        //         assert!(mode.control(abs_con(0.1), &target, ()).is_none());
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.2), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.4), &target, ()).unwrap(),
        //             abs_con(0.5)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.6), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //         assert!(mode.control(abs_con(0.8), &target, ()).is_none());
        //         assert!(mode.control(abs_con(1.0), &target, ()).is_none());
        //     }
        //
        //     #[test]
        //     fn source_interval_out_of_range_min() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             source_value_interval: create_unit_value_interval(0.2, 0.6),
        //             out_of_range_behavior: OutOfRangeBehavior::Min,
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.1), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.2), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.4), &target, ()).unwrap(),
        //             abs_con(0.5)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.6), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.8), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //     }
        //
        //     #[test]
        //     fn source_interval_out_of_range_ignore_source_one_value() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             source_value_interval: create_unit_value_interval(0.5, 0.5),
        //             out_of_range_behavior: OutOfRangeBehavior::Ignore,
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert!(mode.control(abs_con(0.0), &target, ()).is_none());
        //         assert!(mode.control(abs_con(0.4), &target, ()).is_none());
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.5), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //         assert!(mode.control(abs_con(0.6), &target, ()).is_none());
        //         assert!(mode.control(abs_con(1.0), &target, ()).is_none());
        //     }
        //
        //     #[test]
        //     fn source_interval_out_of_range_min_source_one_value() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             source_value_interval: create_unit_value_interval(0.5, 0.5),
        //             out_of_range_behavior: OutOfRangeBehavior::Min,
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.4), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.5), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.6), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //     }
        //
        //     #[test]
        //     fn source_interval_out_of_range_min_max_source_one_value() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             source_value_interval: create_unit_value_interval(0.5, 0.5),
        //             out_of_range_behavior: OutOfRangeBehavior::MinOrMax,
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.4), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.5), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.6), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //     }
        //
        //     #[test]
        //     fn target_interval() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             target_value_interval: create_unit_value_interval(0.2, 0.6),
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(0.2)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.2), &target, ()).unwrap(),
        //             abs_con(0.28)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.25), &target, ()).unwrap(),
        //             abs_con(0.3)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.5), &target, ()).unwrap(),
        //             abs_con(0.4)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.75), &target, ()).unwrap(),
        //             abs_con(0.5)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(0.6)
        //         );
        //     }
        //
        //     #[test]
        //     fn target_interval_reverse() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             target_value_interval: create_unit_value_interval(0.6, 1.0),
        //             reverse: true,
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.25), &target, ()).unwrap(),
        //             abs_con(0.9)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.5), &target, ()).unwrap(),
        //             abs_con(0.8)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.75), &target, ()).unwrap(),
        //             abs_con(0.7)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(0.6)
        //         );
        //     }
        //
        //     #[test]
        //     fn source_and_target_interval() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             source_value_interval: create_unit_value_interval(0.2, 0.6),
        //             target_value_interval: create_unit_value_interval(0.2, 0.6),
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(0.2)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.2), &target, ()).unwrap(),
        //             abs_con(0.2)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.4), &target, ()).unwrap(),
        //             abs_con(0.4)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.6), &target, ()).unwrap(),
        //             abs_con(0.6)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.8), &target, ()).unwrap(),
        //             abs_con(0.6)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(0.6)
        //         );
        //     }
        //
        //     #[test]
        //     fn source_and_target_interval_shifted() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             source_value_interval: create_unit_value_interval(0.2, 0.6),
        //             target_value_interval: create_unit_value_interval(0.4, 0.8),
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(0.4)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.2), &target, ()).unwrap(),
        //             abs_con(0.4)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.4), &target, ()).unwrap(),
        //             abs_con(0.6)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.6), &target, ()).unwrap(),
        //             abs_con(0.8)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.8), &target, ()).unwrap(),
        //             abs_con(0.8)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(0.8)
        //         );
        //     }
        //
        //     #[test]
        //     fn reverse() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             reverse: true,
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.5), &target, ()).unwrap(),
        //             abs_con(0.5)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //     }
        //
        //     #[test]
        //     fn round() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             round_target_value: true,
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteDiscrete {
        //                 atomic_step_size: UnitValue::new(0.2),
        //             },
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.11), &target, ()).unwrap(),
        //             abs_con(0.2)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.19), &target, ()).unwrap(),
        //             abs_con(0.2)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.2), &target, ()).unwrap(),
        //             abs_con(0.2)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.35), &target, ()).unwrap(),
        //             abs_con(0.4)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.49), &target, ()).unwrap(),
        //             abs_con(0.4)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //     }
        //
        //     #[test]
        //     fn jump_interval() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             jump_interval: create_unit_value_interval(0.0, 0.2),
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.5)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert!(mode.control(abs_con(0.0), &target, ()).is_none());
        //         assert!(mode.control(abs_con(0.1), &target, ()).is_none());
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.4), &target, ()).unwrap(),
        //             abs_con(0.4)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.6), &target, ()).unwrap(),
        //             abs_con(0.6)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.7), &target, ()).unwrap(),
        //             abs_con(0.7)
        //         );
        //         assert!(mode.control(abs_con(0.8), &target, ()).is_none());
        //         assert!(mode.control(abs_con(0.9), &target, ()).is_none());
        //         assert!(mode.control(abs_con(1.0), &target, ()).is_none());
        //     }
        //
        //     #[test]
        //     fn jump_interval_min() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             jump_interval: create_unit_value_interval(0.1, 1.0),
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.5)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.1), &target, ()).unwrap(),
        //             abs_con(0.1)
        //         );
        //         assert!(mode.control(abs_con(0.4), &target, ()).is_none());
        //         assert!(mode.control(abs_con(0.5), &target, ()).is_none());
        //         assert!(mode.control(abs_con(0.6), &target, ()).is_none());
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //     }
        //
        //     #[test]
        //     fn jump_interval_approach() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             jump_interval: create_unit_value_interval(0.0, 0.2),
        //             takeover_mode: TakeoverMode::LongTimeNoSee,
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.5)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(0.4)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.1), &target, ()).unwrap(),
        //             abs_con(0.42)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.4), &target, ()).unwrap(),
        //             abs_con(0.4)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.6), &target, ()).unwrap(),
        //             abs_con(0.6)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.7), &target, ()).unwrap(),
        //             abs_con(0.7)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.8), &target, ()).unwrap(),
        //             abs_con(0.56)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(0.6)
        //         );
        //     }
        //
        //     #[test]
        //     fn transformation_ok() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             control_transformation: Some(TestTransformation::new(|input| Ok(input.inverse()))),
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.5), &target, ()).unwrap(),
        //             abs_con(0.5)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //     }
        //
        //     #[test]
        //     fn transformation_err() {
        //         // Given
        //         let mut mode: Mode<TestTransformation> = Mode {
        //             control_transformation: Some(TestTransformation::new(|_| Err("oh no!"))),
        //             ..Default::default()
        //         };
        //         let target = TestTarget {
        //             current_value: Some(continuous_value(0.777)),
        //             control_type: ControlType::AbsoluteContinuous,
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.0), &target, ()).unwrap(),
        //             abs_con(0.0)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(0.5), &target, ()).unwrap(),
        //             abs_con(0.5)
        //         );
        //         assert_abs_diff_eq!(
        //             mode.control(abs_con(1.0), &target, ()).unwrap(),
        //             abs_con(1.0)
        //         );
        //     }
        //
        //     #[test]
        //     fn feedback() {
        //         // Given
        //         let mode: Mode<TestTransformation> = Mode {
        //             ..Default::default()
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(0.5));
        //         assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(1.0));
        //     }
        //
        //     #[test]
        //     fn feedback_reverse() {
        //         // Given
        //         let mode: Mode<TestTransformation> = Mode {
        //             reverse: true,
        //             ..Default::default()
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(1.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(0.5));
        //         assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(0.0));
        //     }
        //
        //     #[test]
        //     fn feedback_target_interval() {
        //         // Given
        //         let mode: Mode<TestTransformation> = Mode {
        //             target_value_interval: create_unit_value_interval(0.2, 1.0),
        //             ..Default::default()
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.2)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.4)).unwrap(), uv(0.25));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.6)).unwrap(), uv(0.5));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.8)).unwrap(), uv(0.75));
        //         assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(1.0));
        //     }
        //
        //     #[test]
        //     fn feedback_target_interval_reverse() {
        //         // Given
        //         let mode: Mode<TestTransformation> = Mode {
        //             target_value_interval: create_unit_value_interval(0.2, 1.0),
        //             reverse: true,
        //             ..Default::default()
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(1.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.2)).unwrap(), uv(1.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.4)).unwrap(), uv(0.75));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.6)).unwrap(), uv(0.5));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.8)).unwrap(), uv(0.25));
        //         assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(0.0));
        //     }
        //
        //     #[test]
        //     fn feedback_source_and_target_interval() {
        //         // Given
        //         let mode: Mode<TestTransformation> = Mode {
        //             source_value_interval: create_unit_value_interval(0.2, 0.8),
        //             target_value_interval: create_unit_value_interval(0.4, 1.0),
        //             ..Default::default()
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.2));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.4)).unwrap(), uv(0.2));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.7)).unwrap(), uv(0.5));
        //         assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(0.8));
        //     }
        //
        //     #[test]
        //     fn feedback_out_of_range_ignore() {
        //         // Given
        //         let mode: Mode<TestTransformation> = Mode {
        //             target_value_interval: create_unit_value_interval(0.2, 0.8),
        //             out_of_range_behavior: OutOfRangeBehavior::Ignore,
        //             ..Default::default()
        //         };
        //         // When
        //         // Then
        //         assert!(mode.feedback(uv(0.0)).is_none());
        //         assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(0.5));
        //         assert!(mode.feedback(uv(1.0)).is_none());
        //     }
        //
        //     #[test]
        //     fn feedback_out_of_range_min() {
        //         // Given
        //         let mode: Mode<TestTransformation> = Mode {
        //             target_value_interval: create_unit_value_interval(0.2, 0.8),
        //             out_of_range_behavior: OutOfRangeBehavior::Min,
        //             ..Default::default()
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.1)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(0.5));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.9)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(0.0));
        //     }
        //
        //     #[test]
        //     fn feedback_out_of_range_min_max_okay() {
        //         // Given
        //         let mode: Mode<TestTransformation> = Mode {
        //             target_value_interval: create_unit_value_interval(0.02, 0.02),
        //             out_of_range_behavior: OutOfRangeBehavior::Min,
        //             ..Default::default()
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.01)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.02)).unwrap(), uv(1.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.03)).unwrap(), uv(0.0));
        //     }
        //
        //     #[test]
        //     fn feedback_out_of_range_min_max_issue_263() {
        //         // Given
        //         let mode: Mode<TestTransformation> = Mode {
        //             target_value_interval: create_unit_value_interval(0.03, 0.03),
        //             out_of_range_behavior: OutOfRangeBehavior::Min,
        //             ..Default::default()
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.01)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.03)).unwrap(), uv(1.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.04)).unwrap(), uv(0.0));
        //     }
        //
        //     #[test]
        //     fn feedback_out_of_range_min_max_issue_263_more() {
        //         // Given
        //         let mode: Mode<TestTransformation> = Mode {
        //             target_value_interval: create_unit_value_interval(0.03, 0.03),
        //             out_of_range_behavior: OutOfRangeBehavior::Min,
        //             ..Default::default()
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.01)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.029999999329447746)).unwrap(), uv(1.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.0300000001)).unwrap(), uv(1.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.04)).unwrap(), uv(0.0));
        //     }
        //
        //     #[test]
        //     fn feedback_out_of_range_min_target_one_value() {
        //         // Given
        //         let mode: Mode<TestTransformation> = Mode {
        //             target_value_interval: create_unit_value_interval(0.5, 0.5),
        //             out_of_range_behavior: OutOfRangeBehavior::Min,
        //             ..Default::default()
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.1)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(1.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.9)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(0.0));
        //     }
        //
        //     #[test]
        //     fn feedback_out_of_range_min_max_target_one_value() {
        //         // Given
        //         let mode: Mode<TestTransformation> = Mode {
        //             target_value_interval: create_unit_value_interval(0.5, 0.5),
        //             ..Default::default()
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.1)).unwrap(), uv(0.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(1.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.9)).unwrap(), uv(1.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(1.0));
        //     }
        //
        //     #[test]
        //     fn feedback_out_of_range_ignore_target_one_value() {
        //         // Given
        //         let mode: Mode<TestTransformation> = Mode {
        //             target_value_interval: create_unit_value_interval(0.5, 0.5),
        //             out_of_range_behavior: OutOfRangeBehavior::Ignore,
        //             ..Default::default()
        //         };
        //         // When
        //         // Then
        //         assert!(mode.feedback(uv(0.0)).is_none());
        //         assert!(mode.feedback(uv(0.1)).is_none());
        //         assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(1.0));
        //         assert!(mode.feedback(uv(0.9)).is_none());
        //         assert!(mode.feedback(uv(1.0)).is_none());
        //     }
        //
        //     #[test]
        //     fn feedback_transformation() {
        //         // Given
        //         let mode: Mode<TestTransformation> = Mode {
        //             feedback_transformation: Some(TestTransformation::new(|input| Ok(input.inverse()))),
        //             ..Default::default()
        //         };
        //         // When
        //         // Then
        //         assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(1.0));
        //         assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(0.5));
        //         assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(0.0));
        //     }
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
                current_value: Some(continuous_value(0.0)),
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
                current_value: Some(continuous_value(1.0)),
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
                current_value: Some(continuous_value(0.333)),
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
                current_value: Some(continuous_value(0.777)),
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
                current_value: Some(continuous_value(0.3)),
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
                current_value: Some(continuous_value(0.7)),
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
                current_value: Some(continuous_value(0.4)),
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
                current_value: Some(continuous_value(0.6)),
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
                current_value: Some(continuous_value(0.0)),
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
                current_value: Some(continuous_value(1.0)),
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
            assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(0.5));
            assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(1.0));
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
            assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
            assert_abs_diff_eq!(mode.feedback(uv(0.4)).unwrap(), uv(0.25));
            assert_abs_diff_eq!(mode.feedback(uv(0.7)).unwrap(), uv(1.0));
            assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(1.0));
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
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
                    current_value: Some(continuous_value(0.2)),
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
                    current_value: Some(continuous_value(0.8)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.199999999999)),
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
                    current_value: Some(continuous_value(0.875)),
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
                    current_value: Some(continuous_value(0.2)),
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
                    current_value: Some(continuous_value(0.8)),
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
                    current_value: Some(continuous_value(0.0)),
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
        }

        mod absolute_discrete_target {
            use super::*;

            #[test]
            fn default_1() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.95));
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
                    current_value: Some(continuous_value(0.0)),
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
                assert_abs_diff_eq!(mode.control(rel(100), &target, ()).unwrap(), abs_con(1.00));
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
                    current_value: Some(continuous_value(1.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.35));
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
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
            fn reverse() {
                // Given
                let mut mode: Mode<TestTransformation> = Mode {
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: Some(continuous_value(0.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.05));
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.95));
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
                    current_value: Some(continuous_value(0.2)),
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
                    current_value: Some(continuous_value(0.8)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.75));
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.2)),
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
                    current_value: Some(continuous_value(0.8)),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target, ()).unwrap(), abs_con(0.75));
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
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
                    current_value: Some(continuous_value(0.990000000001)),
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
                    current_value: Some(continuous_value(0.00999999999999)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.2)),
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
                    current_value: Some(continuous_value(0.8)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.2)),
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
                    current_value: Some(continuous_value(0.8)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(1.0)),
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
                    current_value: Some(continuous_value(0.2)),
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
                    current_value: Some(continuous_value(0.8)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.2)),
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
                    current_value: Some(continuous_value(0.8)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                    current_value: Some(continuous_value(0.0)),
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
                assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.0));
                assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(0.5));
                assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(1.0));
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
                assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(1.0));
                assert_abs_diff_eq!(mode.feedback(uv(0.5)).unwrap(), uv(0.5));
                assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(0.0));
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
                assert_abs_diff_eq!(mode.feedback(uv(0.0)).unwrap(), uv(0.2));
                assert_abs_diff_eq!(mode.feedback(uv(0.4)).unwrap(), uv(0.2));
                assert_abs_diff_eq!(mode.feedback(uv(0.7)).unwrap(), uv(0.5));
                assert_abs_diff_eq!(mode.feedback(uv(1.0)).unwrap(), uv(0.8));
            }
        }
    }

    fn uv(number: f64) -> UnitValue {
        UnitValue::new(number)
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

    fn continuous_value(v: f64) -> AbsoluteValue {
        AbsoluteValue::Continuous(UnitValue::new(v))
    }

    fn discrete_value(actual: u32, max: u32) -> AbsoluteValue {
        AbsoluteValue::Discrete(Fraction::new(actual, max))
    }
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
    Interval::new(0, 99)
}
