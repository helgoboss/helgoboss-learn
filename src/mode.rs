use crate::util::negative_if;
use crate::Instruction::HitTarget;
use crate::{
    ControlValue, DiscreteIncrement, DiscreteValue, Interval, Target, Transformation,
    UnitIncrement, UnitValue,
};

#[derive(Clone, Debug)]
pub enum Mode {
    Absolute(AbsoluteModeData),
    Relative(RelativeModeData),
    Toggle(ToggleModeData),
}

/// Settings for processing control values in absolute mode.
#[derive(Clone, Debug)]
pub struct AbsoluteModeData {
    source_value_interval: Interval<UnitValue>,
    target_value_interval: Interval<UnitValue>,
    jump_interval: Interval<UnitValue>,
    approach_target_value: bool,
    reverse_target_value: bool,
    round_target_value: bool,
    ignore_out_of_range_source_values: bool,
    control_transformation: Option<Transformation>,
    feedback_transformation: Option<Transformation>,
}

impl AbsoluteModeData {
    /// Processes the given control value in absolute mode and returns an appropriate target
    /// instruction.
    pub fn process_control_value(
        &self,
        control_value: UnitValue,
        target: &impl Target,
    ) -> Option<Instruction> {
        if !control_value.is_within_interval(&self.source_value_interval) {
            // Control value is outside source value interval
            if self.ignore_out_of_range_source_values {
                return None;
            }
            let target_bound_value = if control_value < self.source_value_interval.get_min() {
                self.target_value_interval.get_min()
            } else {
                self.target_value_interval.get_max()
            };
            return self.hitting_target_considering_max_jump(target_bound_value, target);
        }
        // Control value is within source value interval
        let pepped_up_control_value = self.pep_up_control_value(control_value, target);
        self.hitting_target_considering_max_jump(pepped_up_control_value, target)
    }

    fn pep_up_control_value(&self, control_value: UnitValue, target: &impl Target) -> UnitValue {
        let mapped_control_value =
            control_value.map_to_unit_interval_from(&self.source_value_interval);
        let transformed_source_value = self
            .control_transformation
            .as_ref()
            .and_then(|t| t.transform(mapped_control_value).ok())
            .unwrap_or(mapped_control_value);
        let mapped_target_value =
            transformed_source_value.map_from_unit_interval_to(&self.target_value_interval);
        let potentially_inversed_target_value = if self.reverse_target_value {
            mapped_target_value.inverse()
        } else {
            mapped_target_value
        };
        if self.round_target_value {
            target.round_to_nearest_discrete_value(potentially_inversed_target_value)
        } else {
            potentially_inversed_target_value
        }
    }

    fn hitting_target_considering_max_jump(
        &self,
        control_value: UnitValue,
        target: &impl Target,
    ) -> Option<Instruction> {
        let current_target_value = target.get_current_value();
        let distance = control_value.calc_distance_from(current_target_value);
        if distance > self.jump_interval.get_max() {
            // Distance too large
            if !self.approach_target_value {
                // Scaling not desired. Do nothing.
                return None;
            }
            // Scaling desired
            let approach_distance = distance.map_from_unit_interval_to(&self.jump_interval);
            let approach_increment = approach_distance
                .to_increment(negative_if(control_value < current_target_value))?;
            let final_target_value =
                current_target_value.add_clamping(approach_increment, &self.target_value_interval);
            return Some(Instruction::hit_absolutely(final_target_value));
        }
        // Distance is not too large
        if distance < self.jump_interval.get_min() {
            return None;
        }
        // Distance is also not too small
        Some(Instruction::hit_absolutely(control_value))
    }
}

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

impl RelativeModeData {
    /// Processes the given control value in relative mode and returns an appropriate target
    /// instruction.
    pub fn process_control_value(
        &self,
        control_value: ControlValue,
        target: &impl Target,
    ) -> Option<Instruction> {
        match control_value {
            ControlValue::Relative(i) => self.process_relative_control_value(i, target),
            ControlValue::Absolute(v) => self.process_absolute_control_value(v, target),
        }
    }

    /// Relative one-direction mode (convert absolute button presses to relative increments)
    fn process_absolute_control_value(
        &self,
        control_value: UnitValue,
        target: &impl Target,
    ) -> Option<Instruction> {
        if control_value.is_zero() || !control_value.is_within_interval(&self.source_value_interval)
        {
            return None;
        }
        if target.wants_to_be_hit_with_increments() {
            // Target wants increments so we just generate them e.g. depending on how hard the
            // button has been pressed
            //
            // - Source value interval (for setting the input interval of relevant source values)
            // - Minimum target step count (enables accurate normal/minimum increment, atomic)
            // - Maximum target step count (enables accurate maximum increment, mapped)
            let discrete_increment = self.convert_to_discrete_increment(control_value)?;
            Some(Instruction::hit_relatively(discrete_increment))
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
    pub fn process_relative_control_value(
        &self,
        discrete_increment: DiscreteIncrement,
        target: &impl Target,
    ) -> Option<Instruction> {
        if target.wants_to_be_hit_with_increments() {
            // Target wants increments so we just forward them after some preprocessing
            //
            // Settings which are always necessary:
            // - Minimum target step count (enables accurate normal/minimum increment, clamped)
            //
            // Settings which are necessary in order to support >1-increments:
            // - Maximum target step count (enables accurate maximum increment, clamped)
            let pepped_up_increment = self.pep_up_discrete_increment(discrete_increment);
            Some(Instruction::hit_relatively(pepped_up_increment))
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
    ) -> Option<Instruction> {
        let unit_increment = discrete_increment.to_unit_increment(atomic_unit_value)?;
        let clamped_unit_increment = unit_increment.clamp_to_interval(&self.step_size_interval);
        Some(self.hitting_target_absolutely_with_unit_increment(clamped_unit_increment, target))
    }

    // TODO Maybe also pass target step size because at least in one case we already have it!
    fn hitting_target_absolutely_with_unit_increment(
        &self,
        increment: UnitIncrement,
        target: &impl Target,
    ) -> Instruction {
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
        Instruction::hit_absolutely(clamped_target_value)
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

#[derive(Clone, Debug)]
pub struct ToggleModeData {
    source_value_interval: Interval<UnitValue>,
    target_value_interval: Interval<UnitValue>,
}

impl ToggleModeData {
    pub fn process_control_value(
        &self,
        control_value: UnitValue,
        target: &impl Target,
    ) -> Option<Instruction> {
        if control_value.is_zero() {
            return None;
        }
        let center_target_value = self.target_value_interval.get_center();
        let bound_target_value = if target.get_current_value() > center_target_value {
            self.target_value_interval.get_min()
        } else {
            self.target_value_interval.get_max()
        };
        Some(Instruction::hit_absolutely(bound_target_value))
    }
}

pub enum Instruction {
    HitTarget(ControlValue),
}

impl Instruction {
    fn hit_absolutely(value: UnitValue) -> Instruction {
        Instruction::HitTarget(ControlValue::Absolute(value))
    }

    fn hit_relatively(increment: DiscreteIncrement) -> Instruction {
        Instruction::HitTarget(ControlValue::Relative(increment))
    }
}

impl Mode {
    pub fn process_control_value(
        &self,
        control_value: ControlValue,
        target: &impl Target,
    ) -> Option<Instruction> {
        use ControlValue::*;
        match self {
            Mode::Absolute(data) => match control_value {
                Absolute(v) => data.process_control_value(v, target),
                Relative(_) => None,
            },
            Mode::Relative(data) => data.process_control_value(control_value, target),
            Mode::Toggle(data) => match control_value {
                Absolute(v) => data.process_control_value(v, target),
                Relative(_) => None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_basic() {
        // Given
        // let mode = Mode::Absolute(AbsoluteModeData {
        //     source_value_interval: Interval::new(NormalizedValue::new(0.0),
        // NormalizedValue::new(1.0)),     target_value_interval: (),
        //     jump_interval: (),
        //     approach_target_value: false,
        //     reverse_target_value: false,
        //     round_target_value: false,
        //     ignore_out_of_interval_source_values: false,
        //     control_transformation: None,
        //     feedback_transformation: None,
        // });
    }
}
