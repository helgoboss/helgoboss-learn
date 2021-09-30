use crate::{AbsoluteValue, UnitValue};
use std::borrow::Cow;

#[derive(Clone, PartialEq, Debug)]
pub enum FeedbackValue<'a> {
    Off,
    Numeric(AbsoluteValue),
    Textual(Cow<'a, str>),
}

impl<'a> FeedbackValue<'a> {
    // TODO-high Check all usages if correct or should process textual as well
    pub fn to_numeric(&self) -> Option<AbsoluteValue> {
        use FeedbackValue::*;
        match self {
            Off => Some(AbsoluteValue::Continuous(UnitValue::MIN)),
            Numeric(v) => Some(*v),
            Textual(_) => None,
        }
    }

    pub fn into_owned(self) -> FeedbackValue<'static> {
        use FeedbackValue::*;
        match self {
            Off => Off,
            Numeric(v) => Numeric(v),
            Textual(v) => Textual(Cow::Owned(v.into_owned())),
        }
    }
}
