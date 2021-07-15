use crate::{AbsoluteValue, ControlType, Target, Transformation};

pub struct TestTarget {
    pub current_value: Option<AbsoluteValue>,
    pub control_type: ControlType,
}

impl<'a> Target<'a> for TestTarget {
    type Context = ();

    fn current_value(&self, _: ()) -> Option<AbsoluteValue> {
        self.current_value
    }

    fn control_type(&self) -> ControlType {
        self.control_type
    }
}

pub struct TestTransformation {
    transformer: Box<dyn Fn(f64) -> Result<f64, &'static str>>,
}

impl TestTransformation {
    pub fn new(
        transformer: impl Fn(f64) -> Result<f64, &'static str> + 'static,
    ) -> TestTransformation {
        Self {
            transformer: Box::new(transformer),
        }
    }
}

impl Transformation for TestTransformation {
    fn transform(&self, input_value: f64, _: f64) -> Result<f64, &'static str> {
        (self.transformer)(input_value)
    }
}
