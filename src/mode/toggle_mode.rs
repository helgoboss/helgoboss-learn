use crate::{full_unit_interval, ControlValue, Interval, Target, UnitValue};

#[derive(Clone, Debug)]
pub struct ToggleMode {
    target_value_interval: Interval<UnitValue>,
}

impl Default for ToggleMode {
    fn default() -> Self {
        ToggleMode {
            target_value_interval: full_unit_interval(),
        }
    }
}

impl ToggleMode {
    /// Processes the given control value in toggle mode and maybe returns an appropriate target
    /// control value.
    pub fn control(&self, control_value: UnitValue, target: &impl Target) -> Option<UnitValue> {
        if control_value.is_zero() {
            return None;
        }
        let center_target_value = self.target_value_interval.get_center();
        let current_target_value = target.get_current_value();
        let desired_target_value = if current_target_value > center_target_value {
            self.target_value_interval.get_min()
        } else {
            self.target_value_interval.get_max()
        };
        if desired_target_value == current_target_value {
            return None;
        }
        Some(desired_target_value)
    }

    /// Takes a target value, interprets and transforms it conforming to toggle mode rules and
    /// returns an appropriate source value that should be sent to the source.
    pub fn feedback(&self, target_value: UnitValue) -> UnitValue {
        // Toggle switches between min and max target value and when doing feedback we want this to translate
        // to min source and max source value. But we also allow feedback of values inbetween. Then users can detect
        // whether a parameter is somewhere between target min and max.
        target_value.map_to_unit_interval_from(&self.target_value_interval)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::create_unit_value_interval;
    use crate::mode::test_util::TestTarget;
    use approx::*;

    #[test]
    fn absolute_value_target_off() {
        // Given
        let mode = ToggleMode {
            ..Default::default()
        };
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(0.0),
            wants_increments: false,
        };
        // When
        // Then
        assert!(mode.control(abs(0.0), &target).is_none());
        assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(1.0));
        assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(1.0));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(1.0));
    }

    #[test]
    fn absolute_value_target_on() {
        // Given
        let mode = ToggleMode {
            ..Default::default()
        };
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
    fn absolute_value_target_rather_off() {
        // Given
        let mode = ToggleMode {
            ..Default::default()
        };
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(0.333),
            wants_increments: false,
        };
        // When
        // Then
        assert!(mode.control(abs(0.0), &target).is_none());
        assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(1.0));
        assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(1.0));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(1.0));
    }

    #[test]
    fn absolute_value_target_rather_on() {
        // Given
        let mode = ToggleMode {
            ..Default::default()
        };
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(0.777),
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
    fn absolute_value_target_interval_target_off() {
        // Given
        let mode = ToggleMode {
            target_value_interval: create_unit_value_interval(0.3, 0.7),
            ..Default::default()
        };
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(0.3),
            wants_increments: false,
        };
        // When
        // Then
        assert!(mode.control(abs(0.0), &target).is_none());
        assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.7));
        assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.7));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.7));
    }

    #[test]
    fn absolute_value_target_interval_target_on() {
        // Given
        let mode = ToggleMode {
            target_value_interval: create_unit_value_interval(0.3, 0.7),
            ..Default::default()
        };
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(0.7),
            wants_increments: false,
        };
        // When
        // Then
        assert!(mode.control(abs(0.0), &target).is_none());
        assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.3));
        assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.3));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.3));
    }

    #[test]
    fn absolute_value_target_interval_target_rather_off() {
        // Given
        let mode = ToggleMode {
            target_value_interval: create_unit_value_interval(0.3, 0.7),
            ..Default::default()
        };
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(0.4),
            wants_increments: false,
        };
        // When
        // Then
        assert!(mode.control(abs(0.0), &target).is_none());
        assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.7));
        assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.7));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.7));
    }

    #[test]
    fn absolute_value_target_interval_target_rather_on() {
        // Given
        let mode = ToggleMode {
            target_value_interval: create_unit_value_interval(0.3, 0.7),
            ..Default::default()
        };
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(0.6),
            wants_increments: false,
        };
        // When
        // Then
        assert!(mode.control(abs(0.0), &target).is_none());
        assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.3));
        assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.3));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.3));
    }

    #[test]
    fn absolute_value_target_interval_target_too_off() {
        // Given
        let mode = ToggleMode {
            target_value_interval: create_unit_value_interval(0.3, 0.7),
            ..Default::default()
        };
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(0.0),
            wants_increments: false,
        };
        // When
        // Then
        assert!(mode.control(abs(0.0), &target).is_none());
        assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.7));
        assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.7));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.7));
    }

    #[test]
    fn absolute_value_target_interval_target_too_on() {
        // Given
        let mode = ToggleMode {
            target_value_interval: create_unit_value_interval(0.3, 0.7),
            ..Default::default()
        };
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(1.0),
            wants_increments: false,
        };
        // When
        // Then
        assert!(mode.control(abs(0.0), &target).is_none());
        assert_abs_diff_eq!(mode.control(abs(0.1), &target).unwrap(), abs(0.3));
        assert_abs_diff_eq!(mode.control(abs(0.5), &target).unwrap(), abs(0.3));
        assert_abs_diff_eq!(mode.control(abs(1.0), &target).unwrap(), abs(0.3));
    }

    #[test]
    fn feedback() {
        // Given
        let mode = ToggleMode {
            ..Default::default()
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.feedback(abs(0.0)), abs(0.0));
        assert_abs_diff_eq!(mode.feedback(abs(0.5)), abs(0.5));
        assert_abs_diff_eq!(mode.feedback(abs(1.0)), abs(1.0));
    }

    #[test]
    fn feedback_target_interval() {
        // Given
        let mode = ToggleMode {
            target_value_interval: create_unit_value_interval(0.3, 0.7),
            ..Default::default()
        };
        // When
        // Then
        assert_abs_diff_eq!(mode.feedback(abs(0.0)), abs(0.0));
        assert_abs_diff_eq!(mode.feedback(abs(0.4)), abs(0.25));
        assert_abs_diff_eq!(mode.feedback(abs(0.7)), abs(1.0));
        assert_abs_diff_eq!(mode.feedback(abs(1.0)), abs(1.0));
    }

    fn abs(number: f64) -> UnitValue {
        UnitValue::new(number)
    }
}
