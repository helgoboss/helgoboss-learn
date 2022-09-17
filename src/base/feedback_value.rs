use crate::{format_percentage_without_unit, AbsoluteValue, RgbColor, UnitValue};
use core::fmt;
use std::borrow::Cow;
use std::fmt::{Display, Formatter};

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum FeedbackValue<'a> {
    Off,
    Numeric(NumericFeedbackValue),
    // This Cow is in case the producer of the feedback value can use the borrowed value. At the
    // moment this is not the case because the target API is designed to return owned strings.
    Textual(TextualFeedbackValue<'a>),
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct NumericFeedbackValue {
    pub style: FeedbackStyle,
    pub value: AbsoluteValue,
}

impl NumericFeedbackValue {
    pub fn new(style: FeedbackStyle, value: AbsoluteValue) -> Self {
        Self { style, value }
    }
}

#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct TextualFeedbackValue<'a> {
    pub style: FeedbackStyle,
    pub text: Cow<'a, str>,
}

impl<'a> TextualFeedbackValue<'a> {
    pub fn new(style: FeedbackStyle, text: Cow<'a, str>) -> Self {
        Self { style, text }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct FeedbackStyle {
    pub color: Option<RgbColor>,
    pub background_color: Option<RgbColor>,
}

impl<'a> FeedbackValue<'a> {
    pub fn to_numeric(&self) -> Option<NumericFeedbackValue> {
        use FeedbackValue::*;
        match self {
            Off => Some(NumericFeedbackValue::new(
                Default::default(),
                AbsoluteValue::Continuous(UnitValue::MIN),
            )),
            Numeric(v) => Some(*v),
            Textual(_) => None,
        }
    }

    pub fn to_textual(&self) -> TextualFeedbackValue {
        use FeedbackValue::*;
        match self {
            Off => Default::default(),
            Numeric(v) => TextualFeedbackValue::new(
                v.style,
                Cow::Owned(format_percentage_without_unit(
                    v.value.to_unit_value().get(),
                )),
            ),
            Textual(v) => TextualFeedbackValue::new(v.style, Cow::Borrowed(v.text.as_ref())),
        }
    }

    pub fn make_owned(self) -> FeedbackValue<'static> {
        use FeedbackValue::*;
        match self {
            Off => Off,
            Numeric(v) => Numeric(v),
            Textual(v) => {
                let new = TextualFeedbackValue::new(v.style, Cow::Owned(v.text.into_owned()));
                FeedbackValue::Textual(new)
            }
        }
    }

    pub fn color(&self) -> Option<RgbColor> {
        use FeedbackValue::*;
        match self {
            Off => None,
            Numeric(v) => v.style.color,
            Textual(v) => v.style.color,
        }
    }

    pub fn background_color(&self) -> Option<RgbColor> {
        use FeedbackValue::*;
        match self {
            Off => None,
            Numeric(v) => v.style.background_color,
            Textual(v) => v.style.background_color,
        }
    }
}

impl<'a> Display for FeedbackValue<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let textual = self.to_textual();
        f.write_str(textual.text.as_ref())?;
        if let Some(c) = textual.style.color {
            write!(f, " with color {}", c)?;
        }
        if let Some(c) = textual.style.background_color {
            write!(f, " with background color {}", c)?;
        }
        Ok(())
    }
}
