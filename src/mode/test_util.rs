use crate::{ControlType, Target, UnitValue};

pub struct TestTarget {
    pub current_value: UnitValue,
    pub control_type: ControlType,
}

impl Target for TestTarget {
    fn current_value(&self) -> UnitValue {
        self.current_value
    }

    fn control_type(&self) -> ControlType {
        self.control_type
    }
}
