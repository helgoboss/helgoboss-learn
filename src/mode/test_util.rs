use crate::{
    AbsoluteValue, ControlType, ControlValueKind, FeedbackScript, FeedbackScriptInput,
    FeedbackScriptOutput, Target, Transformation, TransformationInput, TransformationOutput,
};
use base::hash_util::NonCryptoHashSet;
use std::borrow::Cow;
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
    produced_kind: ControlValueKind,
}

impl TestTransformation {
    pub fn new(
        produced_kind: ControlValueKind,
        transformer: impl Fn(f64) -> Result<f64, &'static str> + 'static,
    ) -> TestTransformation {
        Self {
            transformer: Box::new(transformer),
            produced_kind,
        }
    }
}

impl Transformation for TestTransformation {
    type AdditionalInput = ();

    fn transform(
        &self,
        input: TransformationInput<Self::AdditionalInput>,
    ) -> Result<TransformationOutput, &'static str> {
        let out_val = (self.transformer)(input.event.input_value)?;
        let out = TransformationOutput {
            produced_kind: self.produced_kind,
            value: Some(out_val),
            instruction: None,
        };
        Ok(out)
    }

    fn wants_to_be_polled(&self) -> bool {
        false
    }
}

pub struct TestFeedbackScript;

impl FeedbackScript<'_> for TestFeedbackScript {
    type AdditionalInput = ();

    fn feedback(
        &self,
        _: FeedbackScriptInput,
        _: (),
    ) -> Result<FeedbackScriptOutput, Cow<'static, str>> {
        unimplemented!()
    }

    fn used_props(&self) -> Result<NonCryptoHashSet<String>, Box<dyn Error>> {
        Ok(Default::default())
    }
}
