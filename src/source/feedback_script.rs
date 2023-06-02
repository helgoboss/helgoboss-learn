use crate::{FeedbackValue, PropValue};
use std::borrow::Cow;

pub trait FeedbackScript {
    fn feedback(
        &self,
        input: FeedbackScriptInput,
    ) -> Result<FeedbackScriptOutput, Cow<'static, str>>;

    fn used_props(&self) -> Vec<String>;
}

pub struct FeedbackScriptInput<'a> {
    pub get_prop_value: &'a dyn Fn(&str) -> Option<PropValue>,
}

pub struct FeedbackScriptOutput {
    pub feedback_value: FeedbackValue<'static>,
}
