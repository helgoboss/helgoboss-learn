use crate::{
    format_percentage_without_unit, parse_percentage_without_unit, ControlValue, OscSourceValue,
    SourceCharacter, UnitValue,
};
use rosc::OscType;
use std::convert::TryInto;

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct OscSource {
    address_pattern: String,
}

impl OscSource {
    pub fn new(address_pattern: String) -> Self {
        Self { address_pattern }
    }

    pub fn address_pattern(&self) -> &str {
        &self.address_pattern
    }

    pub fn control(&self, value: OscSourceValue) -> Option<ControlValue> {
        use ControlValue::*;
        let control_value = match value {
            OscSourceValue::Plain(msg) => {
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
                            Int(_) | String(_) | Blob(_) | Time(_) | Long(_) | Char(_)
                            | Color(_) | Midi(_) | Array(_) => return None,
                        }
                    }
                }
            }
        };
        Some(control_value)
    }

    pub fn format_control_value(&self, value: ControlValue) -> Result<String, &'static str> {
        // TODO-high Format depending on custom character
        let absolute_value = value.as_absolute()?;
        Ok(format_percentage_without_unit(absolute_value.get()))
    }

    pub fn parse_control_value(&self, text: &str) -> Result<UnitValue, &'static str> {
        parse_percentage_without_unit(text)?.try_into()
    }

    pub fn character(&self) -> SourceCharacter {
        // TODO-high Add custom character which will also be automatically learned depending on
        //  Bool vs. Inf|Nil vs. Float|Double.
        SourceCharacter::Range
    }

    pub fn feedback(&self, feedback_value: UnitValue) -> Option<OscSourceValue> {
        // TODO-high Create correct source value
        None
    }
}
