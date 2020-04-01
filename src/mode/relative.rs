use crate::{
    create_discrete_value_interval, create_unit_value_interval, negative_if, unit_interval,
    ControlValue, DiscreteIncrement, DiscreteValue, Interval, Target, UnitIncrement, UnitValue,
};

/// Settings for processing control values in relative mode.
#[derive(Clone, Debug)]
pub struct RelativeModeData {
    // TODO Step counts should be display on the right side because they are target-related
    // TODO In ReaLearn, don't display some UI elements, e.g. target min/max
    //  because it doesn't have any influence.
    source_value_interval: Interval<UnitValue>,
    step_count_interval: Interval<DiscreteValue>,
    step_size_interval: Interval<UnitValue>,
    target_value_interval: Interval<UnitValue>,
    reverse: bool,
    rotate: bool,
}

impl Default for RelativeModeData {
    fn default() -> Self {
        RelativeModeData {
            source_value_interval: unit_interval(),
            step_count_interval: create_discrete_value_interval(1, 1),
            step_size_interval: create_unit_value_interval(0.01, 0.01),
            target_value_interval: unit_interval(),
            reverse: false,
            rotate: false,
        }
    }
}

impl RelativeModeData {
    /// Processes the given control value in relative mode and returns an appropriate target
    /// instruction.
    pub fn process(
        &self,
        control_value: ControlValue,
        target: &impl Target,
    ) -> Option<ControlValue> {
        match control_value {
            ControlValue::Relative(i) => self.process_relative(i, target),
            ControlValue::Absolute(v) => self.process_abs(v, target),
        }
    }

    /// Relative one-direction mode (convert absolute button presses to relative increments)
    fn process_abs(&self, control_value: UnitValue, target: &impl Target) -> Option<ControlValue> {
        if control_value.is_zero() || !control_value.is_within_interval(&self.source_value_interval)
        {
            return None;
        }
        if target.wants_increments() {
            // Target wants increments so we just generate them e.g. depending on how hard the
            // button has been pressed
            //
            // - Source value interval (for setting the input interval of relevant source values)
            // - Minimum target step count (enables accurate normal/minimum increment, atomic)
            // - Maximum target step count (enables accurate maximum increment, mapped)
            let discrete_increment = self.convert_to_discrete_increment(control_value)?;
            Some(ControlValue::Relative(discrete_increment))
        } else {
            // Target wants absolute values, so we have to do the incrementation ourselves.
            // That gives us lots of options.
            match target.get_step_size() {
                None => {
                    // Continuous target
                    //
                    // Settings:
                    // - Source value interval (for setting the input interval of relevant source
                    //   values)
                    // - Minimum target step size (enables accurate minimum increment, atomic)
                    // - Maximum target step size (enables accurate maximum increment, clamped)
                    // - Target value interval (absolute, important for rotation only, clamped)
                    let discrete_increment = self.convert_to_discrete_increment(control_value)?;
                    self.hitting_target_absolutely(
                        discrete_increment,
                        self.step_size_interval.get_min(),
                        target,
                    )
                }
                Some(step_size) => {
                    // Discrete target
                    //
                    // Settings:
                    // - Source value interval (for setting the input interval of relevant source
                    //   values)
                    // - Minimum target step count (enables accurate normal/minimum increment,
                    //   atomic)
                    // - Target value interval (absolute, important for rotation only, clamped)
                    // - Maximum target step count (enables accurate maximum increment, clamped)
                    let discrete_increment = self.convert_to_discrete_increment(control_value)?;
                    self.hitting_target_absolutely(discrete_increment, step_size, target)
                }
            }
        }
    }

    fn convert_to_discrete_increment(&self, control_value: UnitValue) -> Option<DiscreteIncrement> {
        let discrete_value = control_value
            .map_to_unit_interval_from(&self.source_value_interval)
            .map_from_unit_interval_to_discrete(&self.step_count_interval);
        discrete_value.to_increment(negative_if(self.reverse))
    }

    // Classic relative mode: We are getting encoder increments from the source.
    // We don't need source min/max config in this case. At least I can't think of a use case
    // where one would like to totally ignore especially slow or especially fast encoder movements,
    // I guess that possibility would rather cause irritation.
    fn process_relative(
        &self,
        discrete_increment: DiscreteIncrement,
        target: &impl Target,
    ) -> Option<ControlValue> {
        if target.wants_increments() {
            // Target wants increments so we just forward them after some preprocessing
            //
            // Settings which are always necessary:
            // - Minimum target step count (enables accurate normal/minimum increment, clamped)
            //
            // Settings which are necessary in order to support >1-increments:
            // - Maximum target step count (enables accurate maximum increment, clamped)
            let pepped_up_increment = self.pep_up_discrete_increment(discrete_increment);
            Some(ControlValue::Relative(pepped_up_increment))
        } else {
            // Target wants absolute values, so we have to do the incrementation ourselves.
            // That gives us lots of options.
            match target.get_step_size() {
                None => {
                    // Continuous target
                    //
                    // Settings which are always necessary:
                    // - Minimum target step size (enables accurate minimum increment, atomic)
                    // - Target value interval (absolute, important for rotation only, clamped)
                    //
                    // Settings which are necessary in order to support >1-increments:
                    // - Maximum target step size (enables accurate maximum increment, clamped)
                    self.hitting_target_absolutely(
                        discrete_increment,
                        self.step_size_interval.get_min(),
                        target,
                    )
                }
                Some(step_size) => {
                    // Discrete target
                    //
                    // Settings which are always necessary:
                    // - Minimum target step count (enables accurate normal/minimum increment,
                    //   atomic)
                    // - Target value interval (absolute, important for rotation only, clamped)
                    //
                    // Settings which are necessary in order to support >1-increments:
                    // - Maximum target step count (enables accurate maximum increment, clamped)
                    let pepped_up_increment = self.pep_up_discrete_increment(discrete_increment);
                    self.hitting_target_absolutely(pepped_up_increment, step_size, target)
                }
            }
        }
    }

    fn hitting_target_absolutely(
        &self,
        discrete_increment: DiscreteIncrement,
        atomic_unit_value: UnitValue,
        target: &impl Target,
    ) -> Option<ControlValue> {
        let unit_increment = discrete_increment.to_unit_increment(atomic_unit_value)?;
        let clamped_unit_increment = unit_increment.clamp_to_interval(&self.step_size_interval);
        Some(self.hitting_target_absolutely_with_unit_increment(clamped_unit_increment, target))
    }

    // TODO Maybe also pass target step size because at least in one case we already have it!
    fn hitting_target_absolutely_with_unit_increment(
        &self,
        increment: UnitIncrement,
        target: &impl Target,
    ) -> ControlValue {
        let current_value = target.get_current_value();
        let incremented_target_value = if self.rotate {
            current_value.add_rotating_at_bounds(increment, &self.target_value_interval)
        } else {
            current_value.add_clamping(increment, &self.target_value_interval)
        };
        let potentially_aligned_value = target
            .get_step_size()
            .map(|step_size| incremented_target_value.round_by_grid_interval_size(step_size))
            .unwrap_or(incremented_target_value);
        let clamped_target_value =
            potentially_aligned_value.clamp_to_interval(&self.target_value_interval);
        ControlValue::Absolute(clamped_target_value)
    }

    fn pep_up_discrete_increment(&self, increment: DiscreteIncrement) -> DiscreteIncrement {
        let clamped_increment = increment.clamp_to_interval(&self.step_count_interval);
        if self.reverse {
            clamped_increment.inverse()
        } else {
            clamped_increment
        }
    }
}
