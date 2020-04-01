use crate::{
    create_discrete_value_interval, create_unit_value_interval, full_unit_interval, negative_if,
    ControlValue, DiscreteIncrement, DiscreteValue, Interval, Target, UnitIncrement, UnitValue,
};

/// Settings for processing control values in relative mode.
#[derive(Clone, Debug)]
pub struct RelativeModeData {
    // TODO Step counts should be display on the right side because they are target-related
    // TODO Don't display source value interval if source emits step counts
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
            source_value_interval: full_unit_interval(),
            // 0.01 has been chosen as default minimum step size because it corresponds to 1%.
            // 0.01 has also been chosen as default maximum step size because most users probably
            // want to start easy, that is without using the "press harder = more increments"
            // respectively "dial harder = more increments" features. Activating them right from
            // the start by choosing a higher step size maximum could lead to surprising results
            // such as ugly parameters jumps, especially if the source is not suited for that.
            step_size_interval: create_unit_value_interval(0.01, 0.01),
            // Same reasoning like with `step_size_interval`
            step_count_interval: create_discrete_value_interval(1, 1),
            target_value_interval: full_unit_interval(),
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
            ControlValue::Absolute(v) => self.process_absolute(v, target),
        }
    }

    /// Relative one-direction mode (convert absolute button presses to relative increments)
    fn process_absolute(
        &self,
        control_value: UnitValue,
        target: &impl Target,
    ) -> Option<ControlValue> {
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
                    let step_size_value = control_value
                        .map_to_unit_interval_from(&self.source_value_interval)
                        .map_from_unit_interval_to(&self.step_size_interval);
                    let step_size_increment =
                        step_size_value.to_increment(negative_if(self.reverse))?;
                    Some(
                        self.hitting_target_absolutely_with_unit_increment(
                            step_size_increment,
                            target,
                        ),
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
                    self.hitting_discrete_target_absolutely(discrete_increment, step_size, target)
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
                    self.hitting_continuous_target_absolutely(
                        if self.reverse {
                            discrete_increment.inverse()
                        } else {
                            discrete_increment
                        },
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
                    self.hitting_discrete_target_absolutely(pepped_up_increment, step_size, target)
                }
            }
        }
    }

    fn hitting_continuous_target_absolutely(
        &self,
        discrete_increment: DiscreteIncrement,
        target: &impl Target,
    ) -> Option<ControlValue> {
        let unit_increment =
            discrete_increment.to_unit_increment(self.step_size_interval.get_min())?;
        let clamped_unit_increment = unit_increment.clamp_to_interval(&self.step_size_interval);
        Some(self.hitting_target_absolutely_with_unit_increment(clamped_unit_increment, target))
    }

    fn hitting_discrete_target_absolutely(
        &self,
        discrete_increment: DiscreteIncrement,
        target_step_size: UnitValue,
        target: &impl Target,
    ) -> Option<ControlValue> {
        let unit_increment = discrete_increment.to_unit_increment(target_step_size)?;
        Some(self.hitting_target_absolutely_with_unit_increment(unit_increment, target))
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
        // If the target has a step size (= has discrete values), we already made sure at this point that the unit increment
        // is an exact multiple of that step size. However, it's possible that the current
        // numerical unit value of the target is in-between two discrete values, not exactly on the
        // perfect discrete value. The target most likely doesn't care and automatically derives the nearest discrete value
        // from that imperfect unit value. However,
        // if we would just apply the increment as-is, we would *again* end up with an imperfect unit value
        // in-between two discrete values. This is not good and could yield weird effects, one being
        // that behavior changes in a non-symmetrical way as soon as target bounds are reached.
        // So we should fix that bad alignment right now and make sure that the target value ends up
        // on a perfect unit value denoting a concrete discrete value.
        // round() is the right choice here because floor() has been found to lead to surprising
        // jumps due to slight numerical inaccuracies.
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::mode::test_util::{abs, rel, TestMode, TestTarget};
    use crate::{create_unit_value_interval, Mode};
    use approx::*;

    mod relative_value {
        use super::*;

        mod absolute_continuous_target {
            use super::*;

            #[test]
            fn default_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.01));
            }

            #[test]
            fn default_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn min_step_size_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.4));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn min_step_size_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.6));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn max_step_size_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.02));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.09));
            }

            #[test]
            fn max_step_size_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.91));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.98));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn reverse() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.0));
                // TODO All those unnecessary target instructions could be avoided because value is same
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.0));
            }

            #[test]
            fn rotate_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.01));
            }

            #[test]
            fn rotate_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.0));
            }

            #[test]
            fn target_interval_min() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.2),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.21));
            }

            #[test]
            fn target_interval_max() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.8),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.79));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.79));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.79));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.8));
            }

            #[test]
            fn target_interval_out() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_min_rotate() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.2),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.21));
            }

            #[test]
            fn target_interval_max_rotate() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.8),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.79));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.79));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.79));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_out_rotate() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.8));
                // TODO This behavior is debatable
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.8));
            }
        }

        mod absolute_discrete_target {
            use super::*;

            #[test]
            fn default_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.05));
            }

            #[test]
            fn default_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.95));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.95));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.95));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn min_step_count_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(4, 100),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(6), &target).unwrap(), abs(0.3));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.5));
            }

            #[test]
            fn min_step_count_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(4, 100),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.5));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn max_step_count_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(1, 2),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.10));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.10));
            }

            #[test]
            fn max_step_count_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(1, 2),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.90));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.90));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.95));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn reverse() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.0));
            }

            #[test]
            fn rotate_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.05));
            }

            #[test]
            fn rotate_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.95));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.95));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.95));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.0));
            }

            #[test]
            fn target_interval_min() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.2),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.25));
            }

            #[test]
            fn target_interval_max() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.8),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.75));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.75));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.75));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.8));
            }

            #[test]
            fn target_interval_out() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_step_interval_out() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(1, 100),
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.2));
                // TODO Not consequent: If the incremented value is high enough, it jumps
                //  from the out-of-range value directly to the incremented value. If not, it
                //  jumps to the interval bound. I can think of two other better behaviors:
                //  a) Even if the incremented value is high enough, jump to interval bound only
                //     (would be more consistent in that it *always* jumps to the bound first).
                //     Preferred variant!
                //  b) Start the increment not from the current out-of-range value but from the
                //     interval bound - always, so not jump to bound first but always use bound
                //     as starting point for the increment.
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.5));
            }

            #[test]
            fn target_interval_min_rotate() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.2),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.25));
            }

            #[test]
            fn target_interval_max_rotate() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.8),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.75));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.75));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.75));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_out_rotate() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(0.8));
            }
        }

        mod relative_target {
            use super::*;

            #[test]
            fn default() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: true,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), rel(1));
            }

            #[test]
            fn min_step_count() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(2, 100),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: true,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), rel(-10));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), rel(-2));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), rel(-2));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), rel(2));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), rel(2));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), rel(10));
            }

            #[test]
            fn max_step_count() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(1, 2),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: true,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), rel(-2));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), rel(-2));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), rel(2));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), rel(2));
            }

            #[test]
            fn reverse() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: true,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), rel(-1));
            }
        }
    }
    mod absolute_value {
        use super::*;
        // TODO Add tests with varying source value intervals

        mod absolute_continuous_target {
            use super::*;

            #[test]
            fn default_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.01));
            }

            #[test]
            fn default_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn min_step_size_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.28));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.6));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn min_step_size_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn max_step_size_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.018));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.process(abs(0.75), &target).unwrap(), abs(0.07));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.09));
            }

            #[test]
            fn max_step_size_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                // TODO Also here, unnecessary target instruction
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn reverse_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.00));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.00));
            }

            #[test]
            fn reverse_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.99));
            }

            #[test]
            fn rotate_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.01));
            }

            #[test]
            fn rotate_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.0));
            }

            #[test]
            fn target_interval_min() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.2),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.21));
            }

            #[test]
            fn target_interval_max() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.8),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.8));
            }

            #[test]
            fn target_interval_out() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_min_rotate() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.2),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.21));
            }

            #[test]
            fn target_interval_max_rotate() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.8),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_out_rotate() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                // TODO This behavior is debatable
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.8));
            }
        }

        mod absolute_discrete_target {
            use super::*;

            #[test]
            fn default_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.05));
            }

            #[test]
            fn default_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn min_step_count_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(4, 8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.3));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.4));
            }

            #[test]
            fn min_step_count_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(4, 8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn max_step_count_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(1, 8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.1));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.4));
            }

            #[test]
            fn max_step_count_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(1, 2),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.process(rel(-10), &target).unwrap(), abs(0.90));
                assert_abs_diff_eq!(mode.process(rel(-2), &target).unwrap(), abs(0.90));
                assert_abs_diff_eq!(mode.process(rel(-1), &target).unwrap(), abs(0.95));
                assert_abs_diff_eq!(mode.process(rel(1), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(2), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(rel(10), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn reverse() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.0));
            }

            #[test]
            fn rotate_1() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.05));
            }

            #[test]
            fn rotate_2() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(1.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.0));
            }

            #[test]
            fn target_interval_min() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.2),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.25));
            }

            #[test]
            fn target_interval_max() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.8),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.8));
            }

            #[test]
            fn target_interval_out() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn step_count_interval_exceeded() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(1, 100),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.55));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn target_interval_step_interval_out() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(1, 100),
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                // TODO Not consequent: See other test (a and b possibilities)
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.55));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.8));
            }

            #[test]
            fn target_interval_min_rotate() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.2),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.25));
            }

            #[test]
            fn target_interval_max_rotate() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.8),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_out_rotate() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                // TODO Behavior debatable
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.8));
            }
        }

        mod relative_target {
            use super::*;

            #[test]
            fn default() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: true,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), rel(1));
            }

            #[test]
            fn min_step_count() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(2, 8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: true,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), rel(3));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), rel(5));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), rel(8));
            }

            #[test]
            fn max_step_count() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    step_count_interval: create_discrete_value_interval(1, 2),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: true,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), rel(2));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), rel(2));
            }

            #[test]
            fn reverse() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    reverse: true,
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: true,
                };
                // When
                // Then
                assert!(mode.process(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), rel(-1));
            }
        }
    }
}
