use crate::{FeedbackValue, PropValue};
use std::borrow::Cow;

pub trait FeedbackScript {
    fn feedback(
        &self,
        input: FeedbackScriptInput,
    ) -> Result<FeedbackScriptOutput, Cow<'static, str>>;

    fn used_props(&self) -> Vec<String>;
}

pub trait PropProvider {
    fn get_prop_value(&self, key: &str) -> Option<PropValue>;
}

impl<F> PropProvider for F
where
    F: Fn(&str) -> Option<PropValue>,
{
    fn get_prop_value(&self, key: &str) -> Option<PropValue> {
        (self)(key)
    }
}

pub struct FeedbackScriptInput<'a> {
    pub prop_provider: &'a dyn PropProvider,
}

pub struct FeedbackScriptOutput {
    pub feedback_value: FeedbackValue<'static>,
}
