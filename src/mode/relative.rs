use crate::{
    create_discrete_value_interval, create_unit_value_interval, full_unit_interval, negative_if,
    ControlValue, DiscreteIncrement, DiscreteValue, Interval, Target, UnitIncrement, UnitValue,
};

/// Settings for processing control values in relative mode.
#[derive(Clone, Debug)]
pub struct RelativeModeData {
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
    /// Processes the given control value in relative mode and maybe returns an appropriate target
    /// control value.
    pub fn control(
        &self,
        control_value: ControlValue,
        target: &impl Target,
    ) -> Option<ControlValue> {
        match control_value {
            ControlValue::Relative(i) => self.process_relative(i, target),
            ControlValue::Absolute(v) => self.process_absolute(v, target),
        }
    }

    /// Takes a target value, interprets and transforms it conforming to relative mode rules and
    /// maybe returns an appropriate source value that should be sent to the source.
    pub fn feedback(&self, target_value: UnitValue) -> Option<UnitValue> {
        let potentially_inversed_value = if self.reverse {
            target_value.inverse()
        } else {
            target_value
        };
        // TODO In case the source emits absolute values, this should be mapped to source value
        //  interval. But how do we know? Maybe we should split the relative mode into two modes?
        Some(potentially_inversed_value.map_to_unit_interval_from(&self.target_value_interval))
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
                    self.hit_target_absolutely_with_unit_increment(
                        step_size_increment,
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
                    self.hit_discrete_target_absolutely(discrete_increment, step_size, target)
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
                    let potentially_reversed_increment = if self.reverse {
                        discrete_increment.inverse()
                    } else {
                        discrete_increment
                    };
                    let unit_increment = potentially_reversed_increment
                        .to_unit_increment(self.step_size_interval.get_min())?;
                    let clamped_unit_increment =
                        unit_increment.clamp_to_interval(&self.step_size_interval);
                    self.hit_target_absolutely_with_unit_increment(
                        clamped_unit_increment,
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
                    self.hit_discrete_target_absolutely(pepped_up_increment, step_size, target)
                }
            }
        }
    }

    fn hit_discrete_target_absolutely(
        &self,
        discrete_increment: DiscreteIncrement,
        target_step_size: UnitValue,
        target: &impl Target,
    ) -> Option<ControlValue> {
        let unit_increment = discrete_increment.to_unit_increment(target_step_size)?;
        self.hit_target_absolutely_with_unit_increment(unit_increment, target_step_size, target)
    }

    fn hit_target_absolutely_with_unit_increment(
        &self,
        increment: UnitIncrement,
        grid_interval_size: UnitValue,
        target: &impl Target,
    ) -> Option<ControlValue> {
        let current_target_value = target.get_current_value();
        // The add functions doesn't add if the current target value is not within the target
        // interval in the first place. Instead it returns one of the interval bounds. One issue
        // that might occur is that the current target value might only *appear* out-of-range
        // because of numerical inaccuracies. That could lead to frustrating "it doesn't move"
        // experiences. Therefore We snap the current target value to grid first.
        let snapped_current_target_value =
            current_target_value.snap_to_grid_by_interval_size(grid_interval_size);
        let incremented_target_value = if self.rotate {
            snapped_current_target_value.add_rotating(increment, &self.target_value_interval)
        } else {
            snapped_current_target_value.add_clamping(increment, &self.target_value_interval)
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
        // on a perfect unit value denoting a concrete discrete value (snap to grid).
        // round() is the right choice here because floor() has been found to lead to surprising
        // jumps due to slight numerical inaccuracies.
        let desired_target_value =
            incremented_target_value.snap_to_grid_by_interval_size(grid_interval_size);
        if desired_target_value == current_target_value {
            return None;
        }
        Some(ControlValue::Absolute(desired_target_value))
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
                assert!(mode.control(rel(-10), &target).is_none());
                assert!(mode.control(rel(-2), &target).is_none());
                assert!(mode.control(rel(-1), &target).is_none());
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.01));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.99));
                assert!(mode.control(rel(1), &target).is_none());
                assert!(mode.control(rel(2), &target).is_none());
                assert!(mode.control(rel(10), &target).is_none());
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
                assert!(mode.control(rel(-10), &target).is_none());
                assert!(mode.control(rel(-2), &target).is_none());
                assert!(mode.control(rel(-1), &target).is_none());
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.4));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(1.0));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.6));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.8));
                assert!(mode.control(rel(1), &target).is_none());
                assert!(mode.control(rel(2), &target).is_none());
                assert!(mode.control(rel(10), &target).is_none());
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
                assert!(mode.control(rel(-10), &target).is_none());
                assert!(mode.control(rel(-2), &target).is_none());
                assert!(mode.control(rel(-1), &target).is_none());
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.02));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.09));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.91));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.98));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.99));
                assert!(mode.control(rel(1), &target).is_none());
                assert!(mode.control(rel(2), &target).is_none());
                assert!(mode.control(rel(10), &target).is_none());
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.01));
                assert!(mode.control(rel(1), &target).is_none());
                assert!(mode.control(rel(2), &target).is_none());
                assert!(mode.control(rel(10), &target).is_none());
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.01));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.0));
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
                assert!(mode.control(rel(-10), &target).is_none());
                assert!(mode.control(rel(-2), &target).is_none());
                assert!(mode.control(rel(-1), &target).is_none());
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.21));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.79));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.79));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.79));
                assert!(mode.control(rel(1), &target).is_none());
                assert!(mode.control(rel(2), &target).is_none());
                assert!(mode.control(rel(10), &target).is_none());
            }

            #[test]
            fn target_interval_current_target_value_out_of_range() {
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_current_target_value_just_appearing_out_of_range() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.199999999999),
                    wants_increments: false,
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.21));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.21));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.79));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.79));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.79));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_rotate_current_target_value_out_of_range() {
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.2));
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
                assert!(mode.control(rel(-10), &target).is_none());
                assert!(mode.control(rel(-2), &target).is_none());
                assert!(mode.control(rel(-1), &target).is_none());
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.05));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.95));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.95));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.95));
                assert!(mode.control(rel(1), &target).is_none());
                assert!(mode.control(rel(2), &target).is_none());
                assert!(mode.control(rel(10), &target).is_none());
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
                assert!(mode.control(rel(-10), &target).is_none());
                assert!(mode.control(rel(-2), &target).is_none());
                assert!(mode.control(rel(-1), &target).is_none());
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(6), &target).unwrap(), abs(0.3));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.5));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.5));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.8));
                assert!(mode.control(rel(1), &target).is_none());
                assert!(mode.control(rel(2), &target).is_none());
                assert!(mode.control(rel(10), &target).is_none());
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
                assert!(mode.control(rel(-10), &target).is_none());
                assert!(mode.control(rel(-2), &target).is_none());
                assert!(mode.control(rel(-1), &target).is_none());
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.10));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.10));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.90));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.90));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.95));
                assert!(mode.control(rel(1), &target).is_none());
                assert!(mode.control(rel(2), &target).is_none());
                assert!(mode.control(rel(10), &target).is_none());
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.05));
                assert!(mode.control(rel(1), &target).is_none());
                assert!(mode.control(rel(2), &target).is_none());
                assert!(mode.control(rel(10), &target).is_none());
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.05));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.95));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.95));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.95));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.0));
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
                assert!(mode.control(rel(-10), &target).is_none());
                assert!(mode.control(rel(-2), &target).is_none());
                assert!(mode.control(rel(-1), &target).is_none());
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.25));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.75));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.75));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.75));
                assert!(mode.control(rel(1), &target).is_none());
                assert!(mode.control(rel(2), &target).is_none());
                assert!(mode.control(rel(10), &target).is_none());
            }

            #[test]
            fn target_interval_current_target_value_out_of_range() {
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_step_interval_current_target_value_out_of_range() {
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.2));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.25));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.75));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.75));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.75));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_rotate_current_target_value_out_of_range() {
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.2));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), rel(1));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), rel(-10));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), rel(-2));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), rel(-2));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), rel(2));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), rel(2));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), rel(10));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), rel(-2));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), rel(-2));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), rel(2));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), rel(2));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), rel(-1));
            }
        }
    }
    mod absolute_value {
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.01));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.5), &target).is_none());
                assert!(mode.control(abs(1.0), &target).is_none());
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.6));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(1.0));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.5), &target).is_none());
                assert!(mode.control(abs(1.0), &target).is_none());
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.02));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(abs(0.75), &target).unwrap(), abs(0.07));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.09));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.5), &target).is_none());
                assert!(mode.control(abs(1.0), &target).is_none());
            }

            #[test]
            fn source_interval() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.25), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.control(abs(0.75), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.01));
            }

            #[test]
            fn source_interval_step_interval() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    step_size_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.25), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.5));
                assert_abs_diff_eq!(mode.control(abs(0.75), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(1.0));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.5), &target).is_none());
                assert!(mode.control(abs(1.0), &target).is_none());
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.99));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.99));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.01));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.01));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.0));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.21));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.1), &target).is_none());
                assert!(mode.control(abs(0.5), &target).is_none());
                assert!(mode.control(abs(1.0), &target).is_none());
            }

            #[test]
            fn target_interval_current_target_value_out_of_range() {
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.2));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.21));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.21));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_rotate_current_target_value_out_of_range() {
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_rotate_reverse_current_target_value_out_of_range() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    reverse: true,
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.8));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.05));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.1), &target).is_none());
                assert!(mode.control(abs(0.5), &target).is_none());
                assert!(mode.control(abs(1.0), &target).is_none());
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.3));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.4));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.1), &target).is_none());
                assert!(mode.control(abs(0.5), &target).is_none());
                assert!(mode.control(abs(1.0), &target).is_none());
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.1));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.4));
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
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.90));
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.90));
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.95));
                assert!(mode.control(rel(1), &target).is_none());
                assert!(mode.control(rel(2), &target).is_none());
                assert!(mode.control(rel(10), &target).is_none());
            }

            #[test]
            fn source_interval() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: Some(UnitValue::new(0.05)),
                    current_value: UnitValue::new(0.0),
                    wants_increments: false,
                };
                // When
                // Then
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.25), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(abs(0.75), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.05));
            }

            #[test]
            fn source_interval_step_interval() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.25), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(0.75), &target).unwrap(), abs(0.3));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.4));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.1), &target).is_none());
                assert!(mode.control(abs(0.5), &target).is_none());
                assert!(mode.control(abs(1.0), &target).is_none());
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.05));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.0));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.0));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.25));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.1), &target).is_none());
                assert!(mode.control(abs(0.5), &target).is_none());
                assert!(mode.control(abs(1.0), &target).is_none());
            }

            #[test]
            fn target_interval_current_target_value_out_of_range() {
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.2));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.55));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(1.0));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn target_interval_step_interval_current_target_value_out_of_range() {
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.2));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.25));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.25));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_rotate_current_target_value_out_of_range() {
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.2));
            }

            #[test]
            fn target_interval_rotate_reverse_current_target_value_out_of_range() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    reverse: true,
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.8));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.8));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), rel(1));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), rel(3));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), rel(5));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), rel(8));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), rel(2));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), rel(2));
            }

            #[test]
            fn source_interval() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: true,
                };
                // When
                // Then
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.25), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), rel(1));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), rel(1));
            }

            #[test]
            fn source_interval_step_interval() {
                // Given
                let mode: TestMode = Mode::Relative(RelativeModeData {
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    step_count_interval: create_discrete_value_interval(4, 8),
                    ..Default::default()
                });
                let target = TestTarget {
                    step_size: None,
                    current_value: UnitValue::new(0.0),
                    wants_increments: true,
                };
                // When
                // Then
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.25), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), rel(4));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), rel(8));
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
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), rel(-1));
            }
        }
    }
}
