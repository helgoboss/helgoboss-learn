use crate::{Target, UnitValue};

pub struct TestTarget {
    pub step_size: Option<UnitValue>,
    pub current_value: UnitValue,
    pub wants_increments: bool,
}

impl Target for TestTarget {
    fn current_value(&self) -> UnitValue {
        self.current_value
    }

    fn step_size(&self) -> Option<UnitValue> {
        self.step_size
    }

    fn wants_increments(&self) -> bool {
        self.wants_increments
    }
}
