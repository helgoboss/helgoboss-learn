use crate::{
    full_unit_interval, mode::feedback_util, negative_if, ControlType, Interval, Lazy, LazyVal,
    PressDurationProcessor, Target, Transformation, UnitValue,
};
use std::time::Duration;

/// Settings for processing control values in absolute mode.
#[derive(Clone, Debug)]
pub struct AbsoluteMode<T: Transformation> {
    pub source_value_interval: Interval<UnitValue>,
    pub target_value_interval: Interval<UnitValue>,
    pub jump_interval: Interval<UnitValue>,
    // TODO-low Not cool to make this public. Maybe derive a builder for this beast.
    pub press_duration_processor: PressDurationProcessor,
    pub approach_target_value: bool,
    pub reverse_target_value: bool,
    pub round_target_value: bool,
    pub ignore_out_of_range_source_values: bool,
    pub control_transformation: Option<T>,
    pub feedback_transformation: Option<T>,
}

impl<T: Transformation> Default for AbsoluteMode<T> {
    fn default() -> Self {
        AbsoluteMode {
            source_value_interval: full_unit_interval(),
            target_value_interval: full_unit_interval(),
            jump_interval: full_unit_interval(),
            press_duration_processor: Default::default(),
            approach_target_value: false,
            reverse_target_value: false,
            round_target_value: false,
            ignore_out_of_range_source_values: false,
            control_transformation: None,
            feedback_transformation: None,
        }
    }
}

impl<T: Transformation> AbsoluteMode<T> {
    /// Processes the given control value in absolute mode and maybe returns an appropriate target
    /// value.
    pub fn control(&mut self, control_value: UnitValue, target: &impl Target) -> Option<UnitValue> {
        let control_value = self.press_duration_processor.process(control_value)?;
        let current_target_value = target.current_value();
        if control_value.is_within_interval(&self.source_value_interval) {
            // Control value is within source value interval
            let pepped_up_control_value =
                self.pep_up_control_value(control_value, target, current_target_value);
            self.hitting_target_considering_max_jump(pepped_up_control_value, current_target_value)
        } else {
            // Control value is outside source value interval
            if self.ignore_out_of_range_source_values {
                return None;
            }
            let target_bound_value = if control_value < self.source_value_interval.min_val() {
                self.target_value_interval.min_val()
            } else {
                self.target_value_interval.max_val()
            };
            return self
                .hitting_target_considering_max_jump(target_bound_value, current_target_value);
        }
    }

    /// Takes a target value, interprets and transforms it conforming to absolute mode rules and
    /// maybe returns an appropriate source value that should be sent to the source.
    pub fn feedback(&self, target_value: UnitValue) -> UnitValue {
        feedback_util::feedback(
            target_value,
            self.reverse_target_value,
            &self.feedback_transformation,
            &self.source_value_interval,
            &self.target_value_interval,
        )
    }

    fn pep_up_control_value(
        &self,
        control_value: UnitValue,
        target: &impl Target,
        current_target_value: UnitValue,
    ) -> UnitValue {
        let mapped_control_value =
            control_value.map_to_unit_interval_from(&self.source_value_interval);
        let transformed_source_value = self
            .control_transformation
            .as_ref()
            .and_then(|t| t.transform(mapped_control_value, current_target_value).ok())
            .unwrap_or(mapped_control_value);
        let mapped_target_value =
            transformed_source_value.map_from_unit_interval_to(&self.target_value_interval);
        let potentially_inversed_target_value = if self.reverse_target_value {
            mapped_target_value.inverse()
        } else {
            mapped_target_value
        };
        if self.round_target_value {
            round_to_nearest_discrete_value(
                &target.control_type(),
                potentially_inversed_target_value,
            )
        } else {
            potentially_inversed_target_value
        }
    }

    fn hitting_target_considering_max_jump(
        &self,
        control_value: UnitValue,
        current_target_value: UnitValue,
    ) -> Option<UnitValue> {
        if self.jump_interval.is_full() {
            // No jump restrictions whatsoever
            return Some(control_value);
        }
        let distance = control_value.calc_distance_from(current_target_value);
        if distance > self.jump_interval.max_val() {
            // Distance is too large
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
            return self.hit_if_changed(final_target_value, current_target_value);
        }
        // Distance is not too large
        if distance < self.jump_interval.min_val() {
            return None;
        }
        // Distance is also not too small
        self.hit_if_changed(control_value, current_target_value)
    }

    fn hit_if_changed(
        &self,
        desired_target_value: UnitValue,
        current_target_value: UnitValue,
    ) -> Option<UnitValue> {
        if current_target_value == desired_target_value {
            return None;
        }
        Some(desired_target_value)
    }
}

fn round_to_nearest_discrete_value(
    control_type: &ControlType,
    approximate_control_value: UnitValue,
) -> UnitValue {
    // round() is the right choice here vs. floor() because we don't want slight numerical
    // inaccuracies lead to surprising jumps
    use ControlType::*;
    let step_size = match control_type {
        AbsoluteContinuousRoundable { rounding_step_size } => *rounding_step_size,
        AbsoluteDiscrete { atomic_step_size } => *atomic_step_size,
        _ => return approximate_control_value,
    };
    approximate_control_value.snap_to_grid_by_interval_size(step_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::create_unit_value_interval;
    use crate::mode::test_util::{TestTarget, TestTransformation};
    use approx::*;

    #[test]
    fn default() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            ..Default::default()
        };
        let target = TestTarget {
            current_value: UnitValue::new(0.777),
            control_type: ControlType::AbsoluteContinuous,
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.control(abs(0.0), &target).unwrap(), abs(0.0));
        assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.5));
        assert_abs_diff_eq!(mode.control(abs(0.777), &target).unwrap(), abs(0.777));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(1.0));
    }

    #[test]
    fn relative_target() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            ..Default::default()
        };
        let target = TestTarget {
            current_value: UnitValue::new(0.777),
            control_type: ControlType::Relative,
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.control(abs(0.0), &target).unwrap(), abs(0.0));
        assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.5));
        assert_abs_diff_eq!(mode.control(abs(0.777), &target).unwrap(), abs(0.777));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(1.0));
    }

    #[test]
    fn source_interval() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            source_value_interval: create_unit_value_interval(0.2, 0.6),
            ..Default::default()
        };
        let target = TestTarget {
            current_value: UnitValue::new(0.777),
            control_type: ControlType::AbsoluteContinuous,
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.control(abs(0.0), &target).unwrap(), abs(0.0));
        assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.0));
        assert_abs_diff_eq!(mode.control(abs(0.2), &target).unwrap(), abs(0.0));
        assert_abs_diff_eq!(mode.control(abs(0.4), &target).unwrap(), abs(0.5));
        assert_abs_diff_eq!(mode.control(abs(0.6), &target).unwrap(), abs(1.0));
        assert_abs_diff_eq!(mode.control(abs(0.8), &target).unwrap(), abs(1.0));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(1.0));
    }

    #[test]
    fn source_interval_ignore() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            source_value_interval: create_unit_value_interval(0.2, 0.6),
            ignore_out_of_range_source_values: true,
            ..Default::default()
        };
        let target = TestTarget {
            current_value: UnitValue::new(0.777),
            control_type: ControlType::AbsoluteContinuous,
        };
        // When
        // Then
        assert!(mode.control(abs(0.0), &target).is_none());
        assert!(mode.control(abs(0.1), &target).is_none());
        assert_abs_diff_eq!(mode.control(abs(0.2), &target).unwrap(), abs(0.0));
        assert_abs_diff_eq!(mode.control(abs(0.4), &target).unwrap(), abs(0.5));
        assert_abs_diff_eq!(mode.control(abs(0.6), &target).unwrap(), abs(1.0));
        assert!(mode.control(abs(0.8), &target).is_none());
        assert!(mode.control(abs(1.0), &target).is_none());
    }

    #[test]
    fn target_interval() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            target_value_interval: create_unit_value_interval(0.2, 0.6),
            ..Default::default()
        };
        let target = TestTarget {
            current_value: UnitValue::new(0.777),
            control_type: ControlType::AbsoluteContinuous,
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.control(abs(0.0), &target).unwrap(), abs(0.2));
        assert_abs_diff_eq!(mode.control(abs(0.2), &target).unwrap(), abs(0.28));
        assert_abs_diff_eq!(mode.control(abs(0.25), &target).unwrap(), abs(0.3));
        assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.4));
        assert_abs_diff_eq!(mode.control(abs(0.75), &target).unwrap(), abs(0.5));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.6));
    }

    #[test]
    fn source_and_target_interval() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            source_value_interval: create_unit_value_interval(0.2, 0.6),
            target_value_interval: create_unit_value_interval(0.2, 0.6),
            ..Default::default()
        };
        let target = TestTarget {
            current_value: UnitValue::new(0.777),
            control_type: ControlType::AbsoluteContinuous,
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.control(abs(0.0), &target).unwrap(), abs(0.2));
        assert_abs_diff_eq!(mode.control(abs(0.2), &target).unwrap(), abs(0.2));
        assert_abs_diff_eq!(mode.control(abs(0.4), &target).unwrap(), abs(0.4));
        assert_abs_diff_eq!(mode.control(abs(0.6), &target).unwrap(), abs(0.6));
        assert_abs_diff_eq!(mode.control(abs(0.8), &target).unwrap(), abs(0.6));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.6));
    }

    #[test]
    fn source_and_target_interval_shifted() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            source_value_interval: create_unit_value_interval(0.2, 0.6),
            target_value_interval: create_unit_value_interval(0.4, 0.8),
            ..Default::default()
        };
        let target = TestTarget {
            current_value: UnitValue::new(0.777),
            control_type: ControlType::AbsoluteContinuous,
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.control(abs(0.0), &target).unwrap(), abs(0.4));
        assert_abs_diff_eq!(mode.control(abs(0.2), &target).unwrap(), abs(0.4));
        assert_abs_diff_eq!(mode.control(abs(0.4), &target).unwrap(), abs(0.6));
        assert_abs_diff_eq!(mode.control(abs(0.6), &target).unwrap(), abs(0.8));
        assert_abs_diff_eq!(mode.control(abs(0.8), &target).unwrap(), abs(0.8));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.8));
    }

    #[test]
    fn reverse() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            reverse_target_value: true,
            ..Default::default()
        };
        let target = TestTarget {
            current_value: UnitValue::new(0.777),
            control_type: ControlType::AbsoluteContinuous,
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.control(abs(0.0), &target).unwrap(), abs(1.0));
        assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.5));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.0));
    }

    #[test]
    fn round() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            round_target_value: true,
            ..Default::default()
        };
        let target = TestTarget {
            current_value: UnitValue::new(0.777),
            control_type: ControlType::AbsoluteDiscrete {
                atomic_step_size: UnitValue::new(0.2),
            },
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.control(abs(0.0), &target).unwrap(), abs(0.0));
        assert_abs_diff_eq!(mode.control(abs(0.11), &target).unwrap(), abs(0.2));
        assert_abs_diff_eq!(mode.control(abs(0.19), &target).unwrap(), abs(0.2));
        assert_abs_diff_eq!(mode.control(abs(0.2), &target).unwrap(), abs(0.2));
        assert_abs_diff_eq!(mode.control(abs(0.35), &target).unwrap(), abs(0.4));
        assert_abs_diff_eq!(mode.control(abs(0.49), &target).unwrap(), abs(0.4));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(1.0));
    }

    #[test]
    fn jump_interval() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            jump_interval: create_unit_value_interval(0.0, 0.2),
            ..Default::default()
        };
        let target = TestTarget {
            current_value: UnitValue::new(0.5),
            control_type: ControlType::AbsoluteContinuous,
        };
        // When
        // Then
        assert!(mode.control(abs(0.0), &target).is_none());
        assert!(mode.control(abs(0.1), &target).is_none());
        assert_abs_diff_eq!(mode.control(abs(0.4), &target).unwrap(), abs(0.4));
        assert_abs_diff_eq!(mode.control(abs(0.6), &target).unwrap(), abs(0.6));
        assert_abs_diff_eq!(mode.control(abs(0.7), &target).unwrap(), abs(0.7));
        assert!(mode.control(abs(0.8), &target).is_none());
        assert!(mode.control(abs(0.9), &target).is_none());
        assert!(mode.control(abs(1.0), &target).is_none());
    }

    #[test]
    fn jump_interval_min() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            jump_interval: create_unit_value_interval(0.1, 1.0),
            ..Default::default()
        };
        let target = TestTarget {
            current_value: UnitValue::new(0.5),
            control_type: ControlType::AbsoluteContinuous,
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.1));
        assert!(mode.control(abs(0.4), &target).is_none());
        assert!(mode.control(abs(0.5), &target).is_none());
        assert!(mode.control(abs(0.6), &target).is_none());
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(1.0));
    }

    #[test]
    fn jump_interval_approach() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            jump_interval: create_unit_value_interval(0.0, 0.2),
            approach_target_value: true,
            ..Default::default()
        };
        let target = TestTarget {
            current_value: UnitValue::new(0.5),
            control_type: ControlType::AbsoluteContinuous,
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.control(abs(0.0), &target).unwrap(), abs(0.4));
        assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.42));
        assert_abs_diff_eq!(mode.control(abs(0.4), &target).unwrap(), abs(0.4));
        assert_abs_diff_eq!(mode.control(abs(0.6), &target).unwrap(), abs(0.6));
        assert_abs_diff_eq!(mode.control(abs(0.7), &target).unwrap(), abs(0.7));
        assert_abs_diff_eq!(mode.control(abs(0.8), &target).unwrap(), abs(0.56));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.6));
    }

    #[test]
    fn transformation_ok() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            control_transformation: Some(TestTransformation::new(|input| Ok(input.inverse()))),
            ..Default::default()
        };
        let target = TestTarget {
            current_value: UnitValue::new(0.777),
            control_type: ControlType::AbsoluteContinuous,
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.control(abs(0.0), &target).unwrap(), abs(1.0));
        assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.5));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.0));
    }

    #[test]
    fn transformation_err() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            control_transformation: Some(TestTransformation::new(|_| Err(()))),
            ..Default::default()
        };
        let target = TestTarget {
            current_value: UnitValue::new(0.777),
            control_type: ControlType::AbsoluteContinuous,
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.control(abs(0.0), &target).unwrap(), abs(0.0));
        assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.5));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(1.0));
    }

    #[test]
    fn feedback() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            ..Default::default()
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.feedback(abs(0.0)), abs(0.0));
        assert_abs_diff_eq!(mode.feedback(abs(0.5)), abs(0.5));
        assert_abs_diff_eq!(mode.feedback(abs(1.0)), abs(1.0));
    }

    #[test]
    fn feedback_reverse() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            reverse_target_value: true,
            ..Default::default()
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.feedback(abs(0.0)), abs(1.0));
        assert_abs_diff_eq!(mode.feedback(abs(0.5)), abs(0.5));
        assert_abs_diff_eq!(mode.feedback(abs(1.0)), abs(0.0));
    }

    #[test]
    fn feedback_source_and_target_interval() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            source_value_interval: create_unit_value_interval(0.2, 0.8),
            target_value_interval: create_unit_value_interval(0.4, 1.0),
            ..Default::default()
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.feedback(abs(0.0)), abs(0.2));
        assert_abs_diff_eq!(mode.feedback(abs(0.4)), abs(0.2));
        assert_abs_diff_eq!(mode.feedback(abs(0.7)), abs(0.5));
        assert_abs_diff_eq!(mode.feedback(abs(1.0)), abs(0.8));
    }

    #[test]
    fn feedback_transformation() {
        // Given
        let mut mode: AbsoluteMode<TestTransformation> = AbsoluteMode {
            feedback_transformation: Some(TestTransformation::new(|input| Ok(input.inverse()))),
            ..Default::default()
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.feedback(abs(0.0)), abs(1.0));
        assert_abs_diff_eq!(mode.feedback(abs(0.5)), abs(0.5));
        assert_abs_diff_eq!(mode.feedback(abs(1.0)), abs(0.0));
    }

    fn abs(number: f64) -> UnitValue {
        UnitValue::new(number)
    }
}
