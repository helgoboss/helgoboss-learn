use crate::{ControlType, Target, Transformation, UnitValue};

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

pub struct TestTransformation {
    transformer: Box<dyn Fn(UnitValue) -> Result<UnitValue, ()>>,
}

impl TestTransformation {
    pub fn new(
        transformer: impl Fn(UnitValue) -> Result<UnitValue, ()> + 'static,
    ) -> TestTransformation {
        Self {
            transformer: Box::new(transformer),
        }
    }
}

impl Transformation for TestTransformation {
    fn transform(&self, input_value: UnitValue) -> Result<UnitValue, ()> {
        (self.transformer)(input_value)
    }
}
