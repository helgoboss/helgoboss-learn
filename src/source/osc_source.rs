use crate::DetailedSourceCharacter::PressOnlyButton;
use crate::{
    format_percentage_without_unit, parse_percentage_without_unit, ControlValue,
    DetailedSourceCharacter, DiscreteIncrement, FeedbackValue, SourceCharacter, UnitValue,
};
use derive_more::Display;
use enum_iterator::IntoEnumIterator;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use rosc::{OscColor, OscMessage, OscType};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::convert::TryInto;

/// With OSC it's easy: The source address is the address!
pub type OscSourceAddress = String;

#[derive(Clone, PartialEq, Debug)]
pub struct OscSource {
    /// To filter out the correct messages.
    address_pattern: String,
    /// To process a value (not just trigger).
    arg_descriptor: Option<OscArgDescriptor>,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct OscArgDescriptor {
    /// To select the correct value.
    index: u32,
    /// To send the correct value type on feedback.
    type_tag: OscTypeTag,
    /// Interpret 1 values as increments and 0 values as decrements.
    is_relative: bool,
}

impl OscArgDescriptor {
    pub fn new(index: u32, type_tag: OscTypeTag, is_relative: bool) -> Self {
        Self {
            index,
            type_tag,
            is_relative,
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
        self.type_tag.to_concrete_args(self.index, value)
    }

    fn from_arg(index: u32, arg: &OscType) -> Self {
        Self {
            index,
            type_tag: OscTypeTag::from_arg(arg),
            // Relative is the exception, so we reset it when learning.
            is_relative: false,
        }
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
    #[display(fmt = "Int (ignored)")]
    Int,
    #[display(fmt = "String (feedback only)")]
    String,
    #[display(fmt = "Blob (ignored)")]
    Blob,
    #[display(fmt = "Time (ignored)")]
    Time,
    #[display(fmt = "Long (ignored)")]
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

    pub fn to_concrete_args(self, index: u32, v: FeedbackValue) -> Option<Vec<OscType>> {
        use OscTypeTag::*;
        let value = match self {
            Float => OscType::Float(v.to_numeric()?.value.to_unit_value().get() as _),
            Double => OscType::Double(v.to_numeric()?.value.to_unit_value().get()),
            Bool => OscType::Bool(v.to_numeric()?.value.is_on()),
            Nil => OscType::Nil,
            Inf => OscType::Inf,
            String => OscType::String(v.to_textual().text.into_owned()),
            Color => {
                let color = match v {
                    FeedbackValue::Off => None,
                    FeedbackValue::Numeric(v) => v.style.color,
                    FeedbackValue::Textual(v) => v.style.color,
                };
                match color {
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
            _ => return None,
        };
        // Send nil for all other elements
        let mut vec = vec![OscType::Nil; (index + 1) as usize];
        vec[index as usize] = value;
        Some(vec)
    }

    pub fn supports_control(self) -> bool {
        use OscTypeTag::*;
        matches!(self, Float | Double | Bool | Nil | Inf)
    }

    pub fn supports_feedback(self) -> bool {
        use OscTypeTag::*;
        matches!(self, Float | Double | Bool | Nil | Inf | String | Color)
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

    pub fn new(address_pattern: String, arg_descriptor: Option<OscArgDescriptor>) -> Self {
        Self {
            address_pattern,
            arg_descriptor,
        }
    }

    pub fn from_source_value(msg: OscMessage, arg_index_hint: Option<u32>) -> OscSource {
        let arg_descriptor = OscArgDescriptor::from_msg(&msg, arg_index_hint.unwrap_or(0));
        OscSource::new(msg.addr, arg_descriptor)
    }

    pub fn address_pattern(&self) -> &str {
        &self.address_pattern
    }

    pub fn arg_descriptor(&self) -> Option<OscArgDescriptor> {
        self.arg_descriptor
    }

    pub fn control(&self, msg: &OscMessage) -> Option<ControlValue> {
        let (unit_value, is_relative) = {
            if msg.addr != self.address_pattern {
                return None;
            }
            if let Some(desc) = self.arg_descriptor {
                if let Some(arg) = msg.args.get(desc.index as usize) {
                    use OscType::*;
                    let v = match arg {
                        Float(f) => UnitValue::new_clamped(*f as _),
                        Double(d) => UnitValue::new(*d),
                        Bool(on) => {
                            if *on {
                                UnitValue::MAX
                            } else {
                                UnitValue::MIN
                            }
                        }
                        // Inifity/impulse or nil/null - act like a trigger.
                        Inf | Nil => UnitValue::MAX,
                        Int(_) | String(_) | Blob(_) | Time(_) | Long(_) | Char(_) | Color(_)
                        | Midi(_) | Array(_) => return None,
                    };
                    (v, desc.is_relative)
                } else {
                    // Argument not found. Don't do anything.
                    return None;
                }
            } else {
                // Source shall not look at any argument. Act like a trigger.
                (UnitValue::MAX, false)
            }
        };
        let control_value = if is_relative {
            let inc = if unit_value.get() > 0.0 { 1 } else { -1 };
            ControlValue::Relative(DiscreteIncrement::new(inc))
        } else {
            ControlValue::AbsoluteContinuous(unit_value)
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
                Float | Double => RangeElement,
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
                    Float | Double => vec![
                        DetailedSourceCharacter::RangeControl,
                        DetailedSourceCharacter::MomentaryVelocitySensitiveButton,
                        DetailedSourceCharacter::MomentaryOnOffButton,
                        DetailedSourceCharacter::PressOnlyButton,
                    ],
                    _ => vec![
                        DetailedSourceCharacter::MomentaryOnOffButton,
                        PressOnlyButton,
                    ],
                }
            }
        } else {
            vec![DetailedSourceCharacter::PressOnlyButton]
        }
    }

    pub fn feedback(&self, feedback_value: FeedbackValue) -> Option<OscMessage> {
        let msg = OscMessage {
            addr: self.address_pattern.clone(),
            args: if let Some(desc) = self.arg_descriptor {
                desc.to_concrete_args(feedback_value)?
            } else {
                // No arguments shall be sent.
                vec![]
            },
        };
        Some(msg)
    }
}
