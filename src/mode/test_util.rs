use crate::{ControlValue, Target, UnitValue};

pub fn abs(number: f64) -> ControlValue {
    ControlValue::absolute(number)
}

pub fn rel(increment: i32) -> ControlValue {
    ControlValue::relative(increment)
}

pub struct TestTarget {
    pub step_size: Option<UnitValue>,
    pub current_value: UnitValue,
    pub wants_increments: bool,
}

impl Target for TestTarget {
    fn get_current_value(&self) -> UnitValue {
        self.current_value
    }

    fn get_step_size(&self) -> Option<UnitValue> {
        self.step_size
    }

    fn wants_increments(&self) -> bool {
        self.wants_increments
    }
}
