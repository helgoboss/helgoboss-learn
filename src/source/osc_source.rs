use crate::{
    format_percentage_without_unit, parse_percentage_without_unit, ControlValue, SourceCharacter,
    UnitValue,
};
use rosc::{OscMessage, OscType};
use std::convert::TryInto;

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct OscSource {
    address_pattern: String,
}

impl OscSource {
    pub fn new(address_pattern: String) -> Self {
        Self { address_pattern }
    }

    pub fn from_source_value(msg: OscMessage) -> OscSource {
        OscSource::new(msg.addr)
    }

    pub fn address_pattern(&self) -> &str {
        &self.address_pattern
    }

    pub fn control(&self, msg: &OscMessage) -> Option<ControlValue> {
        use ControlValue::*;
        let control_value = {
            if msg.addr != self.address_pattern {
                return None;
            }
            match msg.args.first() {
                // No argument - act like a trigger.
                None => Absolute(UnitValue::MAX),
                Some(osc_type) => {
                    use OscType::*;
                    match osc_type {
                        Float(f) => Absolute(UnitValue::new_clamped(*f as _)),
                        Double(d) => Absolute(UnitValue::new(*d)),
                        Bool(on) => Absolute(if *on { UnitValue::MAX } else { UnitValue::MIN }),
                        // Inifity/impulse or nil/null - act like a trigger.
                        Inf | Nil => Absolute(UnitValue::MAX),
                        Int(_) | String(_) | Blob(_) | Time(_) | Long(_) | Char(_) | Color(_)
                        | Midi(_) | Array(_) => return None,
                    }
                }
            }
        };
        Some(control_value)
    }

    pub fn format_control_value(&self, value: ControlValue) -> Result<String, &'static str> {
        // TODO-high (low) Format depending on custom character
        let absolute_value = value.as_absolute()?;
        Ok(format_percentage_without_unit(absolute_value.get()))
    }

    pub fn parse_control_value(&self, text: &str) -> Result<UnitValue, &'static str> {
        parse_percentage_without_unit(text)?.try_into()
    }

    pub fn character(&self) -> SourceCharacter {
        // TODO-high (low) Add custom character which will also be automatically learned depending
        // on  Bool vs. Inf|Nil vs. Float|Double.
        SourceCharacter::Range
    }

    pub fn feedback(&self, feedback_value: UnitValue) -> Option<OscMessage> {
        // TODO-high (low) Use different OscType depending on source character
        // TODO-high (low) Use different value depending on input value
        let msg = OscMessage {
            addr: self.address_pattern.clone(),
            args: vec![OscType::Float(feedback_value.get() as _)],
        };
        Some(msg)
    }
}
