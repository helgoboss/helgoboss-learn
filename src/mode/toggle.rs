use crate::{unit_interval, ControlValue, Interval, Target, UnitValue};

#[derive(Clone, Debug)]
pub struct ToggleModeData {
    source_value_interval: Interval<UnitValue>,
    target_value_interval: Interval<UnitValue>,
}

impl Default for ToggleModeData {
    fn default() -> Self {
        ToggleModeData {
            source_value_interval: unit_interval(),
            target_value_interval: unit_interval(),
        }
    }
}

impl ToggleModeData {
    pub fn process(&self, control_value: UnitValue, target: &impl Target) -> Option<ControlValue> {
        if control_value.is_zero() {
            return None;
        }
        let center_target_value = self.target_value_interval.get_center();
        let bound_target_value = if target.get_current_value() > center_target_value {
            self.target_value_interval.get_min()
        } else {
            self.target_value_interval.get_max()
        };
        Some(ControlValue::Absolute(bound_target_value))
    }
}
