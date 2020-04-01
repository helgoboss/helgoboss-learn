use crate::{
    full_unit_interval, negative_if, ControlValue, Interval, Target, Transformation, UnitValue,
};

/// Settings for processing control values in absolute mode.
#[derive(Clone, Debug)]
pub struct AbsoluteModeData<T: Transformation> {
    source_value_interval: Interval<UnitValue>,
    target_value_interval: Interval<UnitValue>,
    jump_interval: Interval<UnitValue>,
    approach_target_value: bool,
    reverse_target_value: bool,
    round_target_value: bool,
    ignore_out_of_range_source_values: bool,
    control_transformation: Option<T>,
    // TODO Implement feedback logic (in all modes)
    feedback_transformation: Option<T>,
}

impl<T: Transformation> Default for AbsoluteModeData<T> {
    fn default() -> Self {
        AbsoluteModeData {
            source_value_interval: full_unit_interval(),
            target_value_interval: full_unit_interval(),
            jump_interval: full_unit_interval(),
            approach_target_value: false,
            reverse_target_value: false,
            round_target_value: false,
            ignore_out_of_range_source_values: false,
            control_transformation: None,
            feedback_transformation: None,
        }
    }
}

impl<T: Transformation> AbsoluteModeData<T> {
    /// Processes the given control value in absolute mode and returns an appropriate target
    /// instruction.
    pub fn process(&self, control_value: UnitValue, target: &impl Target) -> Option<ControlValue> {
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
            round_to_nearest_discrete_value(target, potentially_inversed_target_value)
        } else {
            potentially_inversed_target_value
        }
    }

    fn hitting_target_considering_max_jump(
        &self,
        control_value: UnitValue,
        target: &impl Target,
    ) -> Option<ControlValue> {
        if self.jump_interval.is_full() {
            // No jump restrictions whatsoever
            return Some(ControlValue::Absolute(control_value));
        }
        let current_target_value = target.get_current_value();
        let distance = control_value.calc_distance_from(current_target_value);
        if distance > self.jump_interval.get_max() {
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
        if distance < self.jump_interval.get_min() {
            return None;
        }
        // Distance is also not too small
        self.hit_if_changed(control_value, current_target_value)
    }

    fn hit_if_changed(
        &self,
        desired_target_value: UnitValue,
        current_target_value: UnitValue,
    ) -> Option<ControlValue> {
        if current_target_value == desired_target_value {
            return None;
        }
        Some(ControlValue::Absolute(desired_target_value))
    }
}

fn round_to_nearest_discrete_value(
    target: &impl Target,
    approximate_control_value: UnitValue,
) -> UnitValue {
    // round() is the right choice here vs. floor() because we don't want slight numerical
    // inaccuracies lead to surprising jumps
    match target.get_step_size() {
        None => approximate_control_value,
        Some(step_size) => approximate_control_value.round_by_grid_interval_size(step_size),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::mode::test_util::{abs, rel, TestMode, TestTarget, TestTransformation};
    use crate::{create_unit_value_interval, Mode};
    use approx::*;

    mod absolute_value {
        use super::*;

        #[test]
        fn default() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                ..Default::default()
            });
            let target = TestTarget {
                step_size: None,
                current_value: UnitValue::new(0.777),
                wants_increments: false,
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.process(abs(0.0), &target).unwrap(), abs(0.0));
            assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.5));
            assert_abs_diff_eq!(mode.process(abs(0.777), &target).unwrap(), abs(0.777));
            assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(1.0));
        }

        #[test]
        fn relative_target() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                ..Default::default()
            });
            let target = TestTarget {
                step_size: None,
                current_value: UnitValue::new(0.777),
                wants_increments: true,
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.process(abs(0.0), &target).unwrap(), abs(0.0));
            assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.5));
            assert_abs_diff_eq!(mode.process(abs(0.777), &target).unwrap(), abs(0.777));
            assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(1.0));
        }

        #[test]
        fn source_interval() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                source_value_interval: create_unit_value_interval(0.2, 0.6),
                ..Default::default()
            });
            let target = TestTarget {
                step_size: None,
                current_value: UnitValue::new(0.777),
                wants_increments: false,
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.process(abs(0.0), &target).unwrap(), abs(0.0));
            assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.0));
            assert_abs_diff_eq!(mode.process(abs(0.2), &target).unwrap(), abs(0.0));
            assert_abs_diff_eq!(mode.process(abs(0.4), &target).unwrap(), abs(0.5));
            assert_abs_diff_eq!(mode.process(abs(0.6), &target).unwrap(), abs(1.0));
            assert_abs_diff_eq!(mode.process(abs(0.8), &target).unwrap(), abs(1.0));
            assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(1.0));
        }

        #[test]
        fn source_interval_ignore() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                source_value_interval: create_unit_value_interval(0.2, 0.6),
                ignore_out_of_range_source_values: true,
                ..Default::default()
            });
            let target = TestTarget {
                step_size: None,
                current_value: UnitValue::new(0.777),
                wants_increments: false,
            };
            // When
            // Then
            assert!(mode.process(abs(0.0), &target).is_none());
            assert!(mode.process(abs(0.1), &target).is_none());
            assert_abs_diff_eq!(mode.process(abs(0.2), &target).unwrap(), abs(0.0));
            assert_abs_diff_eq!(mode.process(abs(0.4), &target).unwrap(), abs(0.5));
            assert_abs_diff_eq!(mode.process(abs(0.6), &target).unwrap(), abs(1.0));
            assert!(mode.process(abs(0.8), &target).is_none());
            assert!(mode.process(abs(1.0), &target).is_none());
        }

        #[test]
        fn target_interval() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                target_value_interval: create_unit_value_interval(0.2, 0.6),
                ..Default::default()
            });
            let target = TestTarget {
                step_size: None,
                current_value: UnitValue::new(0.777),
                wants_increments: false,
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.process(abs(0.0), &target).unwrap(), abs(0.2));
            assert_abs_diff_eq!(mode.process(abs(0.2), &target).unwrap(), abs(0.28));
            assert_abs_diff_eq!(mode.process(abs(0.25), &target).unwrap(), abs(0.3));
            assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.4));
            assert_abs_diff_eq!(mode.process(abs(0.75), &target).unwrap(), abs(0.5));
            assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.6));
        }

        #[test]
        fn source_and_target_interval() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                source_value_interval: create_unit_value_interval(0.2, 0.6),
                target_value_interval: create_unit_value_interval(0.2, 0.6),
                ..Default::default()
            });
            let target = TestTarget {
                step_size: None,
                current_value: UnitValue::new(0.777),
                wants_increments: false,
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.process(abs(0.0), &target).unwrap(), abs(0.2));
            assert_abs_diff_eq!(mode.process(abs(0.2), &target).unwrap(), abs(0.2));
            assert_abs_diff_eq!(mode.process(abs(0.4), &target).unwrap(), abs(0.4));
            assert_abs_diff_eq!(mode.process(abs(0.6), &target).unwrap(), abs(0.6));
            assert_abs_diff_eq!(mode.process(abs(0.8), &target).unwrap(), abs(0.6));
            assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.6));
        }

        #[test]
        fn source_and_target_interval_shifted() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                source_value_interval: create_unit_value_interval(0.2, 0.6),
                target_value_interval: create_unit_value_interval(0.4, 0.8),
                ..Default::default()
            });
            let target = TestTarget {
                step_size: None,
                current_value: UnitValue::new(0.777),
                wants_increments: false,
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.process(abs(0.0), &target).unwrap(), abs(0.4));
            assert_abs_diff_eq!(mode.process(abs(0.2), &target).unwrap(), abs(0.4));
            assert_abs_diff_eq!(mode.process(abs(0.4), &target).unwrap(), abs(0.6));
            assert_abs_diff_eq!(mode.process(abs(0.6), &target).unwrap(), abs(0.8));
            assert_abs_diff_eq!(mode.process(abs(0.8), &target).unwrap(), abs(0.8));
            assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.8));
        }

        #[test]
        fn reverse() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                reverse_target_value: true,
                ..Default::default()
            });
            let target = TestTarget {
                step_size: None,
                current_value: UnitValue::new(0.777),
                wants_increments: false,
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.process(abs(0.0), &target).unwrap(), abs(1.0));
            assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.5));
            assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.0));
        }

        #[test]
        fn round() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                round_target_value: true,
                ..Default::default()
            });
            let target = TestTarget {
                step_size: Some(UnitValue::new(0.2)),
                current_value: UnitValue::new(0.777),
                wants_increments: false,
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.process(abs(0.0), &target).unwrap(), abs(0.0));
            assert_abs_diff_eq!(mode.process(abs(0.11), &target).unwrap(), abs(0.2));
            assert_abs_diff_eq!(mode.process(abs(0.19), &target).unwrap(), abs(0.2));
            assert_abs_diff_eq!(mode.process(abs(0.2), &target).unwrap(), abs(0.2));
            assert_abs_diff_eq!(mode.process(abs(0.35), &target).unwrap(), abs(0.4));
            assert_abs_diff_eq!(mode.process(abs(0.49), &target).unwrap(), abs(0.4));
            assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(1.0));
        }

        #[test]
        fn jump_interval() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                jump_interval: create_unit_value_interval(0.0, 0.2),
                ..Default::default()
            });
            let target = TestTarget {
                step_size: None,
                current_value: UnitValue::new(0.5),
                wants_increments: false,
            };
            // When
            // Then
            assert!(mode.process(abs(0.0), &target).is_none());
            assert!(mode.process(abs(0.1), &target).is_none());
            assert_abs_diff_eq!(mode.process(abs(0.4), &target).unwrap(), abs(0.4));
            assert_abs_diff_eq!(mode.process(abs(0.6), &target).unwrap(), abs(0.6));
            assert_abs_diff_eq!(mode.process(abs(0.7), &target).unwrap(), abs(0.7));
            assert!(mode.process(abs(0.8), &target).is_none());
            assert!(mode.process(abs(0.9), &target).is_none());
            assert!(mode.process(abs(1.0), &target).is_none());
        }

        #[test]
        fn jump_interval_min() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                jump_interval: create_unit_value_interval(0.1, 1.0),
                ..Default::default()
            });
            let target = TestTarget {
                step_size: None,
                current_value: UnitValue::new(0.5),
                wants_increments: false,
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.1));
            assert!(mode.process(abs(0.4), &target).is_none());
            assert!(mode.process(abs(0.5), &target).is_none());
            assert!(mode.process(abs(0.6), &target).is_none());
            assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(1.0));
        }

        #[test]
        fn jump_interval_approach() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                jump_interval: create_unit_value_interval(0.0, 0.2),
                approach_target_value: true,
                ..Default::default()
            });
            let target = TestTarget {
                step_size: None,
                current_value: UnitValue::new(0.5),
                wants_increments: false,
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.process(abs(0.0), &target).unwrap(), abs(0.4));
            assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.42));
            assert_abs_diff_eq!(mode.process(abs(0.4), &target).unwrap(), abs(0.4));
            assert_abs_diff_eq!(mode.process(abs(0.6), &target).unwrap(), abs(0.6));
            assert_abs_diff_eq!(mode.process(abs(0.7), &target).unwrap(), abs(0.7));
            assert_abs_diff_eq!(mode.process(abs(0.8), &target).unwrap(), abs(0.56));
            assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.6));
        }

        #[test]
        fn transformation_ok() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                control_transformation: Some(TestTransformation::new(|input| Ok(input.inverse()))),
                ..Default::default()
            });
            let target = TestTarget {
                step_size: None,
                current_value: UnitValue::new(0.777),
                wants_increments: false,
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.process(abs(0.0), &target).unwrap(), abs(1.0));
            assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.5));
            assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.0));
        }

        #[test]
        fn transformation_err() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                control_transformation: Some(TestTransformation::new(|_| Err(()))),
                ..Default::default()
            });
            let target = TestTarget {
                step_size: None,
                current_value: UnitValue::new(0.777),
                wants_increments: false,
            };
            // When
            // Then
            assert_abs_diff_eq!(mode.process(abs(0.0), &target).unwrap(), abs(0.0));
            assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.5));
            assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(1.0));
        }
    }

    mod relative_value {
        use super::*;

        #[test]
        fn default() {
            // Given
            let mode: TestMode = Mode::Absolute(AbsoluteModeData {
                ..Default::default()
            });
            let target = TestTarget {
                step_size: None,
                current_value: UnitValue::new(0.777),
                wants_increments: false,
            };
            // When
            // Then
            assert!(mode.process(rel(1), &target).is_none());
            assert!(mode.process(rel(-3), &target).is_none());
        }
    }
}
