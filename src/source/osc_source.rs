use crate::DetailedSourceCharacter::Trigger;
use std::cmp;

use crate::{
    format_percentage_without_unit, parse_percentage_without_unit, AbsoluteValue, ControlValue,
    DetailedSourceCharacter, DiscreteIncrement, FeedbackValue, Fraction, Interval, RgbColor,
    SourceCharacter, UnitValue, UNIT_INTERVAL,
};
use derive_more::Display;
use enum_iterator::IntoEnumIterator;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use rosc::{OscColor, OscMessage, OscType};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "serde_with")]
use serde_with::{DeserializeFromStr, SerializeDisplay};
use std::convert::TryInto;
use strum_macros::EnumString;

/// With OSC it's easy: The source address is the address!
pub type OscSourceAddress = String;

#[derive(Clone, PartialEq, Debug)]
pub struct OscSource {
    /// To filter out the correct messages.
    address_pattern: String,
    /// To process a value (not just trigger).
    arg_descriptor: Option<OscArgDescriptor>,
    /// If non-empty, these are used for mapping feedback data to arguments.
    feedback_args: Vec<OscFeedbackProp>,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, EnumString, strum_macros::Display)]
#[cfg_attr(feature = "serde_with", derive(SerializeDisplay, DeserializeFromStr))]
pub enum OscFeedbackProp {
    // Floats
    #[strum(serialize = "value.float")]
    ValueAsFloat,
    // Doubles
    #[strum(serialize = "value.double")]
    ValueAsDouble,
    // Bools
    #[strum(serialize = "value.bool")]
    ValueAsBool,
    // Nil
    #[strum(serialize = "nil")]
    Nil,
    // Inf
    #[strum(serialize = "inf")]
    Inf,
    // Integers
    #[strum(serialize = "value.int")]
    ValueAsInt,
    // Strings
    #[strum(serialize = "value.string")]
    ValueAsString,
    // Longs
    #[strum(serialize = "value.long")]
    ValueAsLong,
    #[strum(serialize = "style.color.rrggbb")]
    ColorRrggbb,
    #[strum(serialize = "style.background_color.rrggbb")]
    BackgroundColorRrggbb,
    // Colors
    #[strum(serialize = "style.color")]
    Color,
    #[strum(serialize = "style.backround_color")]
    BackgroundColor,
}

impl Default for OscFeedbackProp {
    fn default() -> Self {
        Self::Nil
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct OscArgDescriptor {
    /// To select the correct value.
    index: u32,
    /// To send the correct value type on feedback.
    type_tag: OscTypeTag,
    /// Interpret 1 values as increments and 0 values as decrements.
    is_relative: bool,
    /// Value range for all range types (double, float, int, long).
    value_range: Interval<f64>,
}

impl OscArgDescriptor {
    pub fn new(
        index: u32,
        type_tag: OscTypeTag,
        is_relative: bool,
        value_range: Interval<f64>,
    ) -> Self {
        Self {
            index,
            type_tag,
            is_relative,
            value_range,
        }
    }

    pub fn index(self) -> u32 {
        self.index
    }

    pub fn type_tag(self) -> OscTypeTag {
        self.type_tag
    }

    pub fn is_relative(self) -> bool {
        self.is_relative
    }

    pub fn value_range(&self) -> Interval<f64> {
        self.value_range
    }

    pub fn from_msg(msg: &OscMessage, arg_index_hint: u32) -> Option<Self> {
        let desc = if let Some(hinted_arg) = msg.args.get(arg_index_hint as usize) {
            Self::from_arg(arg_index_hint, hinted_arg)
        } else {
            let first_arg = msg.args.first()?;
            Self::from_arg(0, first_arg)
        };
        Some(desc)
    }

    pub fn to_concrete_args(self, value: FeedbackValue) -> Option<Vec<OscType>> {
        self.type_tag
            .to_concrete_args(self.index, value, self.value_range)
    }

    fn from_arg(index: u32, arg: &OscType) -> Self {
        Self {
            index,
            type_tag: OscTypeTag::from_arg(arg),
            // Relative is the exception, so we reset it when learning.
            is_relative: false,
            value_range: match get_range_value(arg) {
                None => DEFAULT_OSC_ARG_VALUE_RANGE,
                Some(v) => Interval::new_auto(0.0, v),
            },
        }
    }
}

pub const DEFAULT_OSC_ARG_VALUE_RANGE: Interval<f64> = UNIT_INTERVAL;

fn get_range_value(arg: &OscType) -> Option<f64> {
    use OscType::*;
    match arg {
        Int(v) => Some(*v as f64),
        Float(v) => Some(*v as f64),
        Long(v) => Some(*v as f64),
        Double(v) => Some(*v),
        _ => None,
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    IntoEnumIterator,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(rename_all = "camelCase")
)]
#[repr(usize)]
// TODO-low Rename. This it not the tag, it's rather the OscType without value.
pub enum OscTypeTag {
    #[display(fmt = "Float")]
    Float,
    #[display(fmt = "Double")]
    Double,
    #[display(fmt = "Bool (on/off)")]
    Bool,
    #[display(fmt = "Nil (trigger only)")]
    Nil,
    #[display(fmt = "Infinitum (trigger only)")]
    Inf,
    #[display(fmt = "Int")]
    Int,
    #[display(fmt = "String (feedback only)")]
    String,
    #[display(fmt = "Blob (ignored)")]
    Blob,
    #[display(fmt = "Time (ignored)")]
    Time,
    #[display(fmt = "Long")]
    Long,
    #[display(fmt = "Char (ignored)")]
    Char,
    #[display(fmt = "Color (feedback only)")]
    Color,
    #[display(fmt = "MIDI (ignored)")]
    Midi,
    #[display(fmt = "Array (ignored)")]
    Array,
}

impl Default for OscTypeTag {
    fn default() -> Self {
        Self::Float
    }
}

impl OscTypeTag {
    pub fn from_arg(arg: &OscType) -> Self {
        use OscType::*;
        match arg {
            Int(_) => Self::Int,
            Float(_) => Self::Float,
            String(_) => Self::String,
            Blob(_) => Self::Blob,
            Time(_) => Self::Time,
            Long(_) => Self::Long,
            Double(_) => Self::Double,
            Char(_) => Self::Char,
            Color(_) => Self::Color,
            Midi(_) => Self::Midi,
            Bool(_) => Self::Bool,
            Array(_) => Self::Array,
            Nil => Self::Nil,
            Inf => Self::Inf,
        }
    }

    pub fn to_concrete_args(
        self,
        index: u32,
        v: FeedbackValue,
        value_range: Interval<f64>,
    ) -> Option<Vec<OscType>> {
        use OscTypeTag::*;
        let value = match self {
            Float => convert_feedback_prop_to_arg(OscFeedbackProp::ValueAsFloat, &v, value_range)?,
            Double => {
                convert_feedback_prop_to_arg(OscFeedbackProp::ValueAsDouble, &v, value_range)?
            }
            Bool => convert_feedback_prop_to_arg(OscFeedbackProp::ValueAsBool, &v, value_range)?,
            Nil => OscType::Nil,
            Inf => OscType::Inf,
            Int => convert_feedback_prop_to_arg(OscFeedbackProp::ValueAsInt, &v, value_range)?,
            String => {
                convert_feedback_prop_to_arg(OscFeedbackProp::ValueAsString, &v, value_range)?
            }
            Long => convert_feedback_prop_to_arg(OscFeedbackProp::ValueAsLong, &v, value_range)?,
            Color => convert_feedback_prop_to_arg(OscFeedbackProp::Color, &v, value_range)?,
            _ => return None,
        };
        // Send nil for all other elements
        let mut vec = vec![OscType::Nil; (index + 1) as usize];
        vec[index as usize] = value;
        Some(vec)
    }

    pub fn supports_control(self) -> bool {
        use OscTypeTag::*;
        matches!(self, Float | Double | Bool | Nil | Inf | Int | Long)
    }

    pub fn supports_feedback(self) -> bool {
        use OscTypeTag::*;
        matches!(
            self,
            Float | Double | Bool | Nil | Inf | Int | String | Long | Color
        )
    }

    pub fn supports_value_range(self) -> bool {
        use OscTypeTag::*;
        matches!(self, Float | Double | Int | Long)
    }

    pub fn is_discrete(self) -> bool {
        use OscTypeTag::*;
        matches!(self, Int | Long)
    }
}

impl OscSource {
    pub fn feedback_address(&self) -> &OscSourceAddress {
        &self.address_pattern
    }

    /// Checks if the given message is directed to the same address as the one of this source.
    ///
    /// Used for:
    ///
    /// -  Source takeover (feedback)
    pub fn has_same_feedback_address_as_value(&self, value: &OscMessage) -> bool {
        self.address_pattern == value.addr
    }

    /// Checks if this and the given source share the same address.
    ///
    /// Used for:
    ///
    /// - Feedback diffing
    pub fn has_same_feedback_address_as_source(&self, other: &Self) -> bool {
        self.address_pattern == other.address_pattern
    }

    pub fn new(
        address_pattern: String,
        arg_descriptor: Option<OscArgDescriptor>,
        feedback_args: Vec<OscFeedbackProp>,
    ) -> Self {
        Self {
            address_pattern,
            arg_descriptor,
            feedback_args,
        }
    }

    pub fn from_source_value(msg: OscMessage, arg_index_hint: Option<u32>) -> OscSource {
        let arg_descriptor = OscArgDescriptor::from_msg(&msg, arg_index_hint.unwrap_or(0));
        OscSource::new(msg.addr, arg_descriptor, vec![])
    }

    pub fn address_pattern(&self) -> &str {
        &self.address_pattern
    }

    pub fn arg_descriptor(&self) -> Option<OscArgDescriptor> {
        self.arg_descriptor
    }

    pub fn control(&self, msg: &OscMessage) -> Option<ControlValue> {
        let (absolute_value, is_relative) = {
            if msg.addr != self.address_pattern {
                return None;
            }
            if let Some(desc) = self.arg_descriptor {
                if let Some(arg) = msg.args.get(desc.index as usize) {
                    use OscType::*;
                    let v =
                        match arg {
                            Float(f) => AbsoluteValue::Continuous(
                                map_continuous_from_range_to_unit(*f as f64, desc.value_range),
                            ),
                            Double(d) => AbsoluteValue::Continuous(
                                map_continuous_from_range_to_unit(*d, desc.value_range),
                            ),
                            Bool(on) => AbsoluteValue::Continuous(if *on {
                                UnitValue::MAX
                            } else {
                                UnitValue::MIN
                            }),
                            // Infinity/impulse or nil/null - act like a trigger.
                            Inf | Nil => AbsoluteValue::Continuous(UnitValue::MAX),
                            Int(i) => AbsoluteValue::Discrete(map_discrete_from_range_to_positive(
                                *i,
                                desc.value_range,
                            )),
                            Long(l) => {
                                // TODO-low-discrete Maybe increase fraction integers to 64-bit? Right now
                                //  we don't really take advantage of fractions, so we emit continuous control
                                //  values as long as this doesn't change.
                                AbsoluteValue::Continuous(map_continuous_from_range_to_unit(
                                    *l as f64,
                                    desc.value_range,
                                ))
                            }
                            String(_) | Blob(_) | Time(_) | Char(_) | Color(_) | Midi(_)
                            | Array(_) => return None,
                        };
                    (v, desc.is_relative)
                } else {
                    // Argument not found. Don't do anything.
                    return None;
                }
            } else {
                // Source shall not look at any argument. Act like a trigger.
                (AbsoluteValue::Continuous(UnitValue::MAX), false)
            }
        };
        let control_value = if is_relative {
            let inc = if absolute_value.is_on() { 1 } else { -1 };
            ControlValue::RelativeDiscrete(DiscreteIncrement::new(inc))
        } else {
            ControlValue::from_absolute(absolute_value)
        };
        Some(control_value)
    }

    pub fn format_control_value(&self, value: ControlValue) -> Result<String, &'static str> {
        let v = value.to_unit_value()?.get();
        Ok(format_percentage_without_unit(v))
    }

    pub fn parse_control_value(&self, text: &str) -> Result<UnitValue, &'static str> {
        parse_percentage_without_unit(text)?.try_into()
    }

    pub fn character(&self) -> SourceCharacter {
        use SourceCharacter::*;
        if let Some(desc) = self.arg_descriptor {
            use OscTypeTag::*;
            match desc.type_tag {
                Float | Double | Int | Long => RangeElement,
                Bool | Nil | Inf => MomentaryButton,
                _ => MomentaryButton,
            }
        } else {
            MomentaryButton
        }
    }

    pub fn possible_detailed_characters(&self) -> Vec<DetailedSourceCharacter> {
        if let Some(desc) = self.arg_descriptor {
            if desc.is_relative {
                vec![DetailedSourceCharacter::Relative]
            } else {
                use OscTypeTag::*;
                match desc.type_tag {
                    Float | Double | Int | Long => vec![
                        DetailedSourceCharacter::RangeControl,
                        DetailedSourceCharacter::MomentaryVelocitySensitiveButton,
                        DetailedSourceCharacter::MomentaryOnOffButton,
                        DetailedSourceCharacter::Trigger,
                    ],
                    _ => vec![DetailedSourceCharacter::MomentaryOnOffButton, Trigger],
                }
            }
        } else {
            vec![DetailedSourceCharacter::Trigger]
        }
    }

    pub fn feedback(&self, feedback_value: FeedbackValue) -> Option<OscMessage> {
        let msg = OscMessage {
            addr: self.address_pattern.clone(),
            args: if !self.feedback_args.is_empty() {
                // Explicit feedback args given.
                let value_range = self
                    .arg_descriptor
                    .map(|desc| desc.value_range)
                    .unwrap_or(DEFAULT_OSC_ARG_VALUE_RANGE);
                self.feedback_args
                    .iter()
                    .map(|prop| {
                        convert_feedback_prop_to_arg(*prop, &feedback_value, value_range)
                            .unwrap_or(OscType::Nil)
                    })
                    .collect()
            } else if let Some(desc) = self.arg_descriptor {
                // No explicit feedback args given. Just derive from argument descriptor.
                desc.to_concrete_args(feedback_value)?
            } else {
                // No arguments shall be sent.
                vec![]
            },
        };
        Some(msg)
    }
}

fn convert_feedback_prop_to_arg(
    prop: OscFeedbackProp,
    v: &FeedbackValue,
    value_range: Interval<f64>,
) -> Option<OscType> {
    use OscFeedbackProp::*;
    let arg = match prop {
        ValueAsFloat | ValueAsDouble | ValueAsLong => {
            let unit_value = v.to_numeric()?.value.to_unit_value();
            let range_value = map_continuous_from_unit_to_range(unit_value, value_range);
            match prop {
                ValueAsFloat => OscType::Float(range_value as _),
                ValueAsDouble => OscType::Double(range_value),
                ValueAsLong => OscType::Long(range_value.round() as i64),
                _ => unreachable!(),
            }
        }
        ValueAsBool => OscType::Bool(v.to_numeric()?.value.is_on()),
        Nil => OscType::Nil,
        Inf => OscType::Inf,
        ValueAsInt => {
            let range_value = match v.to_numeric()?.value {
                AbsoluteValue::Continuous(uv) => {
                    map_continuous_from_unit_to_range(uv, value_range).round() as i32
                }
                AbsoluteValue::Discrete(f) => {
                    map_discrete_from_positive_to_range(f.actual(), value_range)
                }
            };
            OscType::Int(range_value)
        }
        ValueAsString => OscType::String(v.to_textual().text.into_owned()),
        ColorRrggbb => convert_color_to_rrggbb_string_arg(v.color()),
        BackgroundColorRrggbb => convert_color_to_rrggbb_string_arg(v.background_color()),
        Color => convert_color_to_native_color_arg(v.color()),
        BackgroundColor => convert_color_to_native_color_arg(v.background_color()),
    };
    Some(arg)
}

fn convert_color_to_rrggbb_string_arg(v: Option<RgbColor>) -> OscType {
    match v {
        // Nil is hopefully interpreted as "Default color".
        None => OscType::Nil,
        Some(c) => {
            let color_string = format!("{:02X}{:02X}{:02X}", c.r(), c.g(), c.b());
            OscType::String(color_string)
        }
    }
}

fn convert_color_to_native_color_arg(v: Option<RgbColor>) -> OscType {
    match v {
        // Nil is hopefully interpreted as "Default color".
        None => OscType::Nil,
        Some(c) => OscType::Color(OscColor {
            red: c.r(),
            green: c.g(),
            blue: c.b(),
            alpha: 255,
        }),
    }
}

fn map_continuous_from_range_to_unit(x: f64, value_range: Interval<f64>) -> UnitValue {
    // y = (x - min) / span
    let y = (x - value_range.min_val()) / value_range.span();
    UnitValue::new_clamped(y)
}

fn map_continuous_from_unit_to_range(y: UnitValue, value_range: Interval<f64>) -> f64 {
    // y = (x - min) / span
    // y * span = x - min
    // x = y * span + min
    y.get() * value_range.span() + value_range.min_val()
}

fn map_discrete_from_range_to_positive(x: i32, value_range: Interval<f64>) -> Fraction {
    let rounded_range = round_value_range(value_range);
    Fraction::new(
        clamp_to_positive(x - rounded_range.min_val()),
        clamp_to_positive(rounded_range.span()),
    )
}

fn map_discrete_from_positive_to_range(y: u32, value_range: Interval<f64>) -> i32 {
    let rounded_range = round_value_range(value_range);
    y as i32 + rounded_range.min_val()
}

fn round_value_range(value_range: Interval<f64>) -> Interval<i32> {
    Interval::new(
        value_range.min_val().round() as i32,
        value_range.max_val().round() as i32,
    )
}

fn clamp_to_positive(v: i32) -> u32 {
    cmp::max(0, v) as u32
}
