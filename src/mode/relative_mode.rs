use crate::{
    create_discrete_increment_interval, create_unit_value_interval, full_unit_interval,
    mode::feedback_util, negative_if, ControlType, ControlValue, DiscreteIncrement, DiscreteValue,
    Interval, MinIsMaxBehavior, OutOfRangeBehavior, Target, Transformation, UnitIncrement,
    UnitValue,
};

/// Settings for processing control values in relative mode.
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
pub struct RelativeMode<T: Transformation> {
    pub source_value_interval: Interval<UnitValue>,
    /// Negative increments represent fractions (throttling), e.g. -2 fires an increment every
    /// 2nd time only.
    pub step_count_interval: Interval<DiscreteIncrement>,
    pub step_size_interval: Interval<UnitValue>,
    pub target_value_interval: Interval<UnitValue>,
    pub reverse: bool,
    pub rotate: bool,
    /// Counter for implementing throttling.
    ///
    /// Throttling is implemented by spitting out control values only every nth time. The counter
    /// can take positive or negative values in order to detect direction changes. This is positive
    /// when the last change was a positive increment and negative when the last change was a
    /// negative increment.
    pub increment_counter: i32,
    pub feedback_transformation: Option<T>,
    pub out_of_range_behavior: OutOfRangeBehavior,
}

impl<T: Transformation> Default for RelativeMode<T> {
    fn default() -> Self {
        RelativeMode {
            source_value_interval: full_unit_interval(),
            // 0.01 has been chosen as default minimum step size because it corresponds to 1%.
            // 0.01 has also been chosen as default maximum step size because most users probably
            // want to start easy, that is without using the "press harder = more increments"
            // respectively "dial harder = more increments" features. Activating them right from
            // the start by choosing a higher step size maximum could lead to surprising results
            // such as ugly parameters jumps, especially if the source is not suited for that.
            step_size_interval: create_unit_value_interval(0.01, 0.01),
            // Same reasoning like with `step_size_interval`
            step_count_interval: create_discrete_increment_interval(1, 1),
            target_value_interval: full_unit_interval(),
            reverse: false,
            rotate: false,
            increment_counter: 0,
            feedback_transformation: None,
            out_of_range_behavior: OutOfRangeBehavior::MinOrMax,
        }
    }
}

impl<T: Transformation> RelativeMode<T> {
    /// Processes the given control value in relative mode and maybe returns an appropriate target
    /// control value.
    pub fn control(
        &mut self,
        control_value: ControlValue,
        target: &impl Target,
    ) -> Option<ControlValue> {
        match control_value {
            ControlValue::Relative(i) => self.control_relative(i, target),
            ControlValue::Absolute(v) => self.control_absolute(v, target),
        }
    }

    /// Takes a target value, interprets and transforms it conforming to relative mode rules and
    /// returns an appropriate source value that should be sent to the source. Of course this makes
    /// sense for absolute sources only.
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

    /// Relative one-direction mode (convert absolute button presses to relative increments)
    fn control_absolute(
        &mut self,
        control_value: UnitValue,
        target: &impl Target,
    ) -> Option<ControlValue> {
        if control_value.is_zero() || !control_value.is_within_interval(&self.source_value_interval)
        {
            return None;
        }
        use ControlType::*;
        match target.control_type() {
            AbsoluteContinuous | AbsoluteContinuousRoundable { .. } | Virtual => {
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
                    )
                    .map_from_unit_interval_to(&self.step_size_interval);
                let step_size_increment =
                    step_size_value.to_increment(negative_if(self.reverse))?;
                self.hit_target_absolutely_with_unit_increment(
                    step_size_increment,
                    self.step_size_interval.min_val(),
                    target.current_value(),
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
                self.hit_discrete_target_absolutely(discrete_increment, atomic_step_size, || {
                    target.current_value()
                })
            }
            Relative => {
                // Target wants increments so we just generate them e.g. depending on how hard the
                // button has been pressed
                //
                // - Source value interval (for setting the input interval of relevant source
                //   values)
                // - Minimum target step count (enables accurate normal/minimum increment, atomic)
                // - Maximum target step count (enables accurate maximum increment, mapped)
                let discrete_increment = self.convert_to_discrete_increment(control_value)?;
                Some(ControlValue::Relative(discrete_increment))
            }
        }
    }

    fn convert_to_discrete_increment(
        &mut self,
        control_value: UnitValue,
    ) -> Option<DiscreteIncrement> {
        let factor = control_value
            .map_to_unit_interval_from(&self.source_value_interval, MinIsMaxBehavior::PreferOne)
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

    // Classic relative mode: We are getting encoder increments from the source.
    // We don't need source min/max config in this case. At least I can't think of a use case
    // where one would like to totally ignore especially slow or especially fast encoder movements,
    // I guess that possibility would rather cause irritation.
    fn control_relative(
        &mut self,
        discrete_increment: DiscreteIncrement,
        target: &impl Target,
    ) -> Option<ControlValue> {
        use ControlType::*;
        match target.control_type() {
            AbsoluteContinuous | AbsoluteContinuousRoundable { .. } => {
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
                    unit_increment.clamp_to_interval(&self.step_size_interval);
                self.hit_target_absolutely_with_unit_increment(
                    clamped_unit_increment,
                    self.step_size_interval.min_val(),
                    target.current_value(),
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
                self.hit_discrete_target_absolutely(pepped_up_increment, atomic_step_size, || {
                    target.current_value()
                })
            }
            Relative | Virtual => {
                // Target wants increments so we just forward them after some preprocessing
                //
                // Settings which are always necessary:
                // - Minimum target step count (enables accurate normal/minimum increment, clamped)
                //
                // Settings which are necessary in order to support >1-increments:
                // - Maximum target step count (enables accurate maximum increment, clamped)
                let pepped_up_increment = self.pep_up_discrete_increment(discrete_increment)?;
                Some(ControlValue::Relative(pepped_up_increment))
            }
        }
    }

    fn hit_discrete_target_absolutely(
        &self,
        discrete_increment: DiscreteIncrement,
        target_step_size: UnitValue,
        current_value: impl Fn() -> UnitValue,
    ) -> Option<ControlValue> {
        let unit_increment = discrete_increment.to_unit_increment(target_step_size)?;
        self.hit_target_absolutely_with_unit_increment(
            unit_increment,
            target_step_size,
            current_value(),
        )
    }

    fn hit_target_absolutely_with_unit_increment(
        &self,
        increment: UnitIncrement,
        grid_interval_size: UnitValue,
        current_target_value: UnitValue,
    ) -> Option<ControlValue> {
        // The add functions doesn't add if the current target value is not within the target
        // interval in the first place. Instead it returns one of the interval bounds. One issue
        // that might occur is that the current target value might only *appear* out-of-range
        // because of numerical inaccuracies. That could lead to frustrating "it doesn't move"
        // experiences. Therefore We snap the current target value to grid first.
        let snapped_current_target_value =
            current_target_value.snap_to_grid_by_interval_size(grid_interval_size);
        let snapped_target_value_interval = Interval::new(
            self.target_value_interval
                .min_val()
                .snap_to_grid_by_interval_size(grid_interval_size),
            self.target_value_interval
                .max_val()
                .snap_to_grid_by_interval_size(grid_interval_size),
        );
        let desired_target_value = if self.rotate {
            snapped_current_target_value.add_rotating(increment, &snapped_target_value_interval)
        } else {
            snapped_current_target_value.add_clamping(increment, &snapped_target_value_interval)
        };
        if desired_target_value == current_target_value {
            return None;
        }
        Some(ControlValue::Absolute(desired_target_value))
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
        if self.increment_counter.signum() != direction_signum {
            // Change of direction. In this case always fire.
            return (true, direction_signum);
        }
        let positive_increment_counter = self.increment_counter.abs() as u32;
        if positive_increment_counter >= nth {
            // After having waited for a few increments, fire again.
            return (true, direction_signum);
        }
        (false, self.increment_counter + direction_signum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::create_unit_value_interval;
    use crate::mode::test_util::{TestTarget, TestTransformation};
    use approx::*;

    mod relative_value {
        use super::*;

        mod absolute_continuous_target {
            use super::*;

            #[test]
            fn default_1() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.2),
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.8),
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.199999999999),
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.2),
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.8),
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(4, 100),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(rel(-10), &target).is_none());
                assert!(mode.control(rel(-2), &target).is_none());
                assert!(mode.control(rel(-1), &target).is_none());
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.20)); // 4x
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.25)); // 5x
                assert_abs_diff_eq!(mode.control(rel(4), &target).unwrap(), abs(0.35)); // 7x
                assert_abs_diff_eq!(mode.control(rel(10), &target).unwrap(), abs(0.65)); // 13x
                assert_abs_diff_eq!(mode.control(rel(100), &target).unwrap(), abs(1.00)); // 100x
            }

            #[test]
            fn min_step_count_2() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(4, 100),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert_abs_diff_eq!(mode.control(rel(-10), &target).unwrap(), abs(0.35)); // 13x
                assert_abs_diff_eq!(mode.control(rel(-2), &target).unwrap(), abs(0.75)); // 5x
                assert_abs_diff_eq!(mode.control(rel(-1), &target).unwrap(), abs(0.8)); // 4x
                assert!(mode.control(rel(1), &target).is_none());
                assert!(mode.control(rel(2), &target).is_none());
                assert!(mode.control(rel(10), &target).is_none());
            }

            #[test]
            fn max_step_count_1() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(1, 2),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
            fn max_step_count_throttle() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(-2, -2),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                // No effect because already min
                assert!(mode.control(rel(-10), &target).is_none());
                assert!(mode.control(rel(-10), &target).is_none());
                assert!(mode.control(rel(-10), &target).is_none());
                assert!(mode.control(rel(-10), &target).is_none());
                // Every 2nd time
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.05));
                assert!(mode.control(rel(1), &target).is_none());
                assert_abs_diff_eq!(mode.control(rel(1), &target).unwrap(), abs(0.05));
                assert!(mode.control(rel(2), &target).is_none());
                assert_abs_diff_eq!(mode.control(rel(2), &target).unwrap(), abs(0.05));
            }

            #[test]
            fn max_step_count_2() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(1, 2),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.2),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.8),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(1, 100),
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.2),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.8),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_eq!(mode.control(rel(-10), &target), Some(rel(-1)));
                assert_eq!(mode.control(rel(-2), &target), Some(rel(-1)));
                assert_eq!(mode.control(rel(-1), &target), Some(rel(-1)));
                assert_eq!(mode.control(rel(1), &target), Some(rel(1)));
                assert_eq!(mode.control(rel(2), &target), Some(rel(1)));
                assert_eq!(mode.control(rel(10), &target), Some(rel(1)));
            }

            #[test]
            fn min_step_count() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(2, 100),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_eq!(mode.control(rel(-10), &target), Some(rel(-11)));
                assert_eq!(mode.control(rel(-2), &target), Some(rel(-3)));
                assert_eq!(mode.control(rel(-1), &target), Some(rel(-2)));
                assert_eq!(mode.control(rel(1), &target), Some(rel(2)));
                assert_eq!(mode.control(rel(2), &target), Some(rel(3)));
                assert_eq!(mode.control(rel(10), &target), Some(rel(11)));
            }

            #[test]
            fn min_step_count_throttle() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(-4, 100),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                // So intense that reaching speedup area
                assert_eq!(mode.control(rel(-10), &target), Some(rel(-6)));
                // Every 3rd time
                assert_eq!(mode.control(rel(-2), &target), Some(rel(-1)));
                assert_eq!(mode.control(rel(-2), &target), None);
                assert_eq!(mode.control(rel(-2), &target), None);
                assert_eq!(mode.control(rel(-2), &target), Some(rel(-1)));
                // Every 4th time (but fired before)
                assert_eq!(mode.control(rel(-1), &target), None);
                assert_eq!(mode.control(rel(-1), &target), None);
                assert_eq!(mode.control(rel(-1), &target), None);
                assert_eq!(mode.control(rel(-1), &target), Some(rel(-1)));
                // Direction change
                assert_eq!(mode.control(rel(1), &target), Some(rel(1)));
                // Every 3rd time (but fired before)
                assert_eq!(mode.control(rel(2), &target), None);
                assert_eq!(mode.control(rel(2), &target), None);
                assert_eq!(mode.control(rel(2), &target), Some(rel(1)));
                // So intense that reaching speedup area
                assert_eq!(mode.control(rel(10), &target), Some(rel(6)));
            }

            #[test]
            fn max_step_count() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(1, 2),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_eq!(mode.control(rel(-10), &target), Some(rel(-2)));
                assert_eq!(mode.control(rel(-2), &target), Some(rel(-2)));
                assert_eq!(mode.control(rel(-1), &target), Some(rel(-1)));
                assert_eq!(mode.control(rel(1), &target), Some(rel(1)));
                assert_eq!(mode.control(rel(2), &target), Some(rel(2)));
                assert_eq!(mode.control(rel(10), &target), Some(rel(2)));
            }

            #[test]
            fn max_step_count_throttle() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(-10, -4),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                // Every 4th time
                assert_eq!(mode.control(rel(-10), &target), Some(rel(-1)));
                assert_eq!(mode.control(rel(-10), &target), None);
                assert_eq!(mode.control(rel(-10), &target), None);
                assert_eq!(mode.control(rel(-10), &target), None);
                assert_eq!(mode.control(rel(-10), &target), Some(rel(-1)));
                assert_eq!(mode.control(rel(-10), &target), None);
                assert_eq!(mode.control(rel(-10), &target), None);
                assert_eq!(mode.control(rel(-10), &target), None);
                // Every 10th time
                assert_eq!(mode.control(rel(1), &target), Some(rel(1)));
                assert_eq!(mode.control(rel(1), &target), None);
                assert_eq!(mode.control(rel(1), &target), None);
                assert_eq!(mode.control(rel(1), &target), None);
                assert_eq!(mode.control(rel(1), &target), None);
                assert_eq!(mode.control(rel(1), &target), None);
                assert_eq!(mode.control(rel(1), &target), None);
                assert_eq!(mode.control(rel(1), &target), None);
                assert_eq!(mode.control(rel(1), &target), None);
                assert_eq!(mode.control(rel(1), &target), None);
                assert_eq!(mode.control(rel(1), &target), Some(rel(1)));
            }

            #[test]
            fn reverse() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert_eq!(mode.control(rel(-10), &target), Some(rel(1)));
                assert_eq!(mode.control(rel(-2), &target), Some(rel(1)));
                assert_eq!(mode.control(rel(-1), &target), Some(rel(1)));
                assert_eq!(mode.control(rel(1), &target), Some(rel(-1)));
                assert_eq!(mode.control(rel(2), &target), Some(rel(-1)));
                assert_eq!(mode.control(rel(10), &target), Some(rel(-1)));
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.28));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.6));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn min_step_size_2() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_size_interval: create_unit_value_interval(0.2, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.018));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.05));
                assert_abs_diff_eq!(mode.control(abs(0.75), &target).unwrap(), abs(0.07));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.09));
            }

            #[test]
            fn max_step_size_2() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_size_interval: create_unit_value_interval(0.01, 0.09),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    step_size_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
                };
                // When
                // Then
                assert!(mode.control(abs(0.0), &target).is_none());
                assert!(mode.control(abs(0.25), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.5));
                assert_abs_diff_eq!(mode.control(abs(0.75), &target).unwrap(), abs(0.75));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(1.0));
            }

            #[test]
            fn reverse_1() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.2),
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.8),
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.2),
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.8),
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    reverse: true,
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteContinuous,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(4, 8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.2));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.3));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.4));
            }

            #[test]
            fn min_step_count_throttle() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(-4, -4),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
                };
                // When
                // Then
                assert!(mode.control(abs(0.0), &target).is_none());
                // Every 4th time
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.05));
                assert!(mode.control(abs(0.1), &target).is_none());
                assert!(mode.control(abs(0.1), &target).is_none());
                assert!(mode.control(abs(0.1), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.05));
            }

            #[test]
            fn min_step_count_2() {
                // Given
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(4, 8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(1, 8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(1, 2),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    step_count_interval: create_discrete_increment_interval(4, 8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MAX,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.2),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.8),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(1, 100),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(1, 100),
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.2),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::new(0.8),
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    target_value_interval: create_unit_value_interval(0.2, 0.8),
                    reverse: true,
                    rotate: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::AbsoluteDiscrete {
                        atomic_step_size: UnitValue::new(0.05),
                    },
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::Relative,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(2, 8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::Relative,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    step_count_interval: create_discrete_increment_interval(1, 2),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::Relative,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::Relative,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    source_value_interval: create_unit_value_interval(0.5, 1.0),
                    step_count_interval: create_discrete_increment_interval(4, 8),
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::Relative,
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
                let mut mode: RelativeMode<TestTransformation> = RelativeMode {
                    reverse: true,
                    ..Default::default()
                };
                let target = TestTarget {
                    current_value: UnitValue::MIN,
                    control_type: ControlType::Relative,
                };
                // When
                // Then
                assert!(mode.control(abs(0.0), &target).is_none());
                assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), rel(-1));
                assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), rel(-1));
            }
        }

        mod feedback {
            use super::*;

            #[test]
            fn default() {
                // Given
                let mode: RelativeMode<TestTransformation> = RelativeMode {
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
                let mode: RelativeMode<TestTransformation> = RelativeMode {
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
                let mode: RelativeMode<TestTransformation> = RelativeMode {
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

    fn abs(number: f64) -> ControlValue {
        ControlValue::absolute(number)
    }

    fn rel(increment: i32) -> ControlValue {
        ControlValue::relative(increment)
    }

    fn uv(number: f64) -> UnitValue {
        UnitValue::new(number)
    }
}
