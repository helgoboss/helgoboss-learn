use crate::{FeedbackValue, PropValue};
use base::hash_util::NonCryptoHashSet;
use std::borrow::Cow;
use std::error::Error;

// The lifetime 'a is necessary in case we want to parameterize the lifetime
// of the additional input dynamically. An alternative would have been to
// require the additional input type to be static and take it by reference.
// But that would be less generic.
pub trait FeedbackScript<'a> {
    type AdditionalInput: Default;

    fn feedback(
        &self,
        input: FeedbackScriptInput,
        additional_input: Self::AdditionalInput,
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
