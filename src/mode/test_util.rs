use crate::{
    AbsoluteValue, ControlType, FeedbackScript, FeedbackScriptInput, FeedbackScriptOutput, Target,
    Transformation, TransformationInput, TransformationOutput,
};
use std::borrow::Cow;
use std::collections::HashSet;
use std::error::Error;

pub struct TestTarget {
    pub current_value: Option<AbsoluteValue>,
    pub control_type: ControlType,
}

impl<'a> Target<'a> for TestTarget {
    type Context = ();

    fn current_value(&self, _: ()) -> Option<AbsoluteValue> {
        self.current_value
    }

    fn control_type(&self, _: ()) -> ControlType {
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
    type AdditionalInput = ();

    fn transform(
        &self,
        input: TransformationInput<f64>,
        _: f64,
        _: (),
    ) -> Result<TransformationOutput<f64>, &'static str> {
        (self.transformer)(input.value).map(TransformationOutput::Control)
    }

    fn wants_to_be_polled(&self) -> bool {
        false
    }
}

pub struct TestFeedbackScript;

impl FeedbackScript for TestFeedbackScript {
    fn feedback(&self, _: FeedbackScriptInput) -> Result<FeedbackScriptOutput, Cow<'static, str>> {
        unimplemented!()
    }

    fn used_props(&self) -> Result<NonCryptoHashSet<String>, Box<dyn Error>> {
        Ok(Default::default())
    }
}
