use crate::{format_percentage_without_unit, AbsoluteValue, UnitValue};
use core::fmt;
use std::borrow::Cow;
use std::fmt::{Display, Formatter};

#[derive(Clone, PartialEq, Debug)]
pub enum FeedbackValue<'a> {
    Off,
    Numeric(AbsoluteValue),
    Textual(Cow<'a, str>),
}

impl<'a> FeedbackValue<'a> {
    pub fn to_numeric(&self) -> Option<AbsoluteValue> {
        use FeedbackValue::*;
        match self {
            Off => Some(AbsoluteValue::Continuous(UnitValue::MIN)),
            Numeric(v) => Some(*v),
            Textual(_) => None,
        }
    }

    pub fn to_textual(&self) -> Cow<str> {
        use FeedbackValue::*;
        match self {
            Off => Cow::default(),
            Numeric(v) => Cow::Owned(format_percentage_without_unit(v.to_unit_value().get())),
            Textual(text) => Cow::Borrowed(text.as_ref()),
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

impl<'a> Display for FeedbackValue<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_textual().as_ref())
    }
}
