use crate::{FeedbackValue, PropValue};
use base::hash_util::NonCryptoHashSet;
use std::borrow::Cow;
use std::error::Error;

pub trait FeedbackScript {
    fn feedback(
        &self,
        input: FeedbackScriptInput,
    ) -> Result<FeedbackScriptOutput, Cow<'static, str>>;

    fn used_props(&self) -> Result<NonCryptoHashSet<String>, Box<dyn Error>>;
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
