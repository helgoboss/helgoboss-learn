use crate::{
    format_percentage_without_unit, parse_percentage_without_unit, ControlValue, DiscreteIncrement,
    SourceCharacter, UnitValue,
};
use derive_more::Display;
use enum_iterator::IntoEnumIterator;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use rosc::{OscMessage, OscType};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::convert::TryInto;

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
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

    fn from_arg(index: u32, arg: &OscType) -> Self {
        Self {
            index: index,
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
    #[display(fmt = "String (ignored)")]
    String,
    #[display(fmt = "Blob (ignored)")]
    Blob,
    #[display(fmt = "Time (ignored)")]
    Time,
    #[display(fmt = "Long (ignored)")]
    Long,
    #[display(fmt = "Char (ignored)")]
    Char,
    #[display(fmt = "Color (ignored)")]
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
}

impl OscSource {
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
            ControlValue::Absolute(unit_value)
        };
        Some(control_value)
    }

    pub fn format_control_value(&self, value: ControlValue) -> Result<String, &'static str> {
        let v = value.as_absolute()?.get();
        let res = if let Some(desc) = self.arg_descriptor {
            use OscTypeTag::*;
            match desc.type_tag {
                Float | Double => format_percentage_without_unit(v),
                Bool => (if v == 0.0 { "off" } else { "on" }).to_owned(),
                Nil | Inf => "Trigger".to_owned(),
                _ => return Err("no way to interpret value with such an OSC type tag"),
            }
        } else {
            "Trigger".to_owned()
        };
        Ok(res)
    }

    pub fn parse_control_value(&self, text: &str) -> Result<UnitValue, &'static str> {
        parse_percentage_without_unit(text)?.try_into()
    }

    pub fn character(&self) -> SourceCharacter {
        use SourceCharacter::*;
        if let Some(desc) = self.arg_descriptor {
            use OscTypeTag::*;
            match desc.type_tag {
                Float | Double => Range,
                Bool | Nil | Inf => Button,
                _ => Button,
            }
        } else {
            Button
        }
    }

    pub fn feedback(&self, feedback_value: UnitValue) -> Option<OscMessage> {
        let v = feedback_value.get();
        let args = {
            if let Some(desc) = self.arg_descriptor {
                use OscTypeTag::*;
                let value = match desc.type_tag {
                    Float => OscType::Float(v as _),
                    Double => OscType::Double(v),
                    Bool => OscType::Bool(v > 0.0),
                    Nil => OscType::Nil,
                    Inf => OscType::Inf,
                    _ => return None,
                };
                // Send nil for all other elements
                let mut vec = vec![OscType::Inf; (desc.index + 1) as usize];
                vec[desc.index as usize] = value;
                vec
            } else {
                // No arguments shall be sent.
                vec![]
            }
        };
        let msg = OscMessage {
            addr: self.address_pattern.clone(),
            args,
        };
        Some(msg)
    }
}
