use crate::{DiscreteIncrement, UnitValue};

/// Value coming from a source (e.g. a MIDI source) which is supposed to control something.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ControlValue {
    /// Absolute value (e.g. fader position, knob position, key press, key release)
    Absolute(UnitValue),
    /// Relative value (e.g. encoder movement)
    Relative(DiscreteIncrement),
}

impl ControlValue {
    /// Convenience method for creating an absolute control value
    pub fn absolute(number: f64) -> ControlValue {
        ControlValue::Absolute(UnitValue::new(number))
    }

    /// Convenience method for creating a relative control value
    pub fn relative(increment: i32) -> ControlValue {
        ControlValue::Relative(DiscreteIncrement::new(increment))
    }

    /// Extracts the unit value if this is an absolute control value.
    pub fn as_absolute(self) -> Result<UnitValue, &'static str> {
        match self {
            ControlValue::Absolute(v) => Ok(v),
            _ => Err("control value is not absolute"),
        }
    }

    /// Extracts the discrete increment if this is a relative control value.
    pub fn as_relative(self) -> Result<DiscreteIncrement, &'static str> {
        match self {
            ControlValue::Relative(v) => Ok(v),
            _ => Err("control value is not relative"),
        }
    }

    pub fn inverse(self) -> ControlValue {
        match self {
            ControlValue::Absolute(v) => ControlValue::Absolute(v.inverse()),
            ControlValue::Relative(v) => ControlValue::Relative(v.inverse()),
        }
    }
}
