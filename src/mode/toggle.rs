use crate::{full_unit_interval, ControlValue, Interval, Target, UnitValue};

#[derive(Clone, Debug)]
pub struct ToggleModeData {
    target_value_interval: Interval<UnitValue>,
}

impl Default for ToggleModeData {
    fn default() -> Self {
        ToggleModeData {
            target_value_interval: full_unit_interval(),
        }
    }
}

impl ToggleModeData {
    pub fn process(&self, control_value: UnitValue, target: &impl Target) -> Option<ControlValue> {
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
        Some(ControlValue::Absolute(desired_target_value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::mode::test_util::{abs, rel, TestMode, TestTarget};
    use crate::{create_unit_value_interval, Mode};
    use approx::*;

    #[test]
    fn absolute_value_target_off() {
        // Given
        let mode: TestMode = Mode::Toggle(ToggleModeData {
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
        assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(1.0));
        assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(1.0));
        assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(1.0));
    }

    #[test]
    fn relative_value() {
        // Given
        let mode: TestMode = Mode::Toggle(ToggleModeData {
            ..Default::default()
        });
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(0.0),
            wants_increments: false,
        };
        // When
        // Then
        assert!(mode.process(rel(1), &target).is_none());
        assert!(mode.process(rel(-1), &target).is_none());
    }

    #[test]
    fn absolute_value_target_on() {
        // Given
        let mode: TestMode = Mode::Toggle(ToggleModeData {
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
    fn absolute_value_target_rather_off() {
        // Given
        let mode: TestMode = Mode::Toggle(ToggleModeData {
            ..Default::default()
        });
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(0.333),
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
    fn absolute_value_target_rather_on() {
        // Given
        let mode: TestMode = Mode::Toggle(ToggleModeData {
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
        assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.0));
        assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.0));
        assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.0));
    }

    #[test]
    fn absolute_value_target_interval_target_off() {
        // Given
        let mode: TestMode = Mode::Toggle(ToggleModeData {
            target_value_interval: create_unit_value_interval(0.3, 0.7),
            ..Default::default()
        });
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(0.3),
            wants_increments: false,
        };
        // When
        // Then
        assert!(mode.process(abs(0.0), &target).is_none());
        assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.7));
        assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.7));
        assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.7));
    }

    #[test]
    fn absolute_value_target_interval_target_on() {
        // Given
        let mode: TestMode = Mode::Toggle(ToggleModeData {
            target_value_interval: create_unit_value_interval(0.3, 0.7),
            ..Default::default()
        });
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(0.7),
            wants_increments: false,
        };
        // When
        // Then
        assert!(mode.process(abs(0.0), &target).is_none());
        assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.3));
        assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.3));
        assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.3));
    }

    #[test]
    fn absolute_value_target_interval_target_rather_off() {
        // Given
        let mode: TestMode = Mode::Toggle(ToggleModeData {
            target_value_interval: create_unit_value_interval(0.3, 0.7),
            ..Default::default()
        });
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(0.4),
            wants_increments: false,
        };
        // When
        // Then
        assert!(mode.process(abs(0.0), &target).is_none());
        assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.7));
        assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.7));
        assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.7));
    }

    #[test]
    fn absolute_value_target_interval_target_rather_on() {
        // Given
        let mode: TestMode = Mode::Toggle(ToggleModeData {
            target_value_interval: create_unit_value_interval(0.3, 0.7),
            ..Default::default()
        });
        let target = TestTarget {
            step_size: None,
            current_value: UnitValue::new(0.6),
            wants_increments: false,
        };
        // When
        // Then
        assert!(mode.process(abs(0.0), &target).is_none());
        assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.3));
        assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.3));
        assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.3));
    }

    #[test]
    fn absolute_value_target_interval_target_too_off() {
        // Given
        let mode: TestMode = Mode::Toggle(ToggleModeData {
            target_value_interval: create_unit_value_interval(0.3, 0.7),
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
        assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.7));
        assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.7));
        assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.7));
    }

    #[test]
    fn absolute_value_target_interval_target_too_on() {
        // Given
        let mode: TestMode = Mode::Toggle(ToggleModeData {
            target_value_interval: create_unit_value_interval(0.3, 0.7),
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
        assert_abs_diff_eq!(mode.process(abs(0.1), &target).unwrap(), abs(0.3));
        assert_abs_diff_eq!(mode.process(abs(0.5), &target).unwrap(), abs(0.3));
        assert_abs_diff_eq!(mode.process(abs(1.0), &target).unwrap(), abs(0.3));
    }
}
