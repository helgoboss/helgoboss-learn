use crate::{DiscreteIncrement, Fraction, Interval, UnitValue};
use std::ops::Deref;

/// Value coming from a source (e.g. a MIDI source) which is supposed to control something.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ControlValue {
    /// Absolute value that represents a percentage (e.g. fader position on the scale from lowest to
    /// highest, knob position on the scale from closed to fully opened, key press on the scale from
    /// not pressed to pressed with full velocity, key release).
    AbsoluteContinuous(UnitValue),
    /// Absolute value that is capable of retaining the original discrete value, e.g. the played
    /// note number, without immediately converting it into a UnitValue and thereby losing that
    /// information - which is important for the new "Discrete" mode.
    AbsoluteDiscrete(Fraction),
    /// Relative increment (e.g. encoder movement)
    Relative(DiscreteIncrement),
}

impl ControlValue {
    /// Convenience method for creating an absolute control value
    pub fn absolute_continuous(number: f64) -> ControlValue {
        ControlValue::AbsoluteContinuous(UnitValue::new(number))
    }

    /// Convenience method for creating a discrete absolute control value
    pub fn absolute_discrete(actual: u32, max: u32) -> ControlValue {
        ControlValue::AbsoluteDiscrete(Fraction::new(actual, max))
    }

    /// Convenience method for creating a relative control value
    pub fn relative(increment: i32) -> ControlValue {
        ControlValue::Relative(DiscreteIncrement::new(increment))
    }

    /// Extracts the unit value if this is an absolute control value.
    pub fn as_unit_value(self) -> Result<UnitValue, &'static str> {
        match self {
            ControlValue::AbsoluteContinuous(v) => Ok(v),
            ControlValue::AbsoluteDiscrete(f) => Ok(f.into()),
            _ => Err("control value is not absolute"),
        }
    }

    /// Extracts the discrete increment if this is a relative control value.
    pub fn as_discrete_increment(self) -> Result<DiscreteIncrement, &'static str> {
        match self {
            ControlValue::Relative(v) => Ok(v),
            _ => Err("control value is not relative"),
        }
    }

    pub fn inverse(self) -> ControlValue {
        match self {
            ControlValue::AbsoluteContinuous(v) => ControlValue::AbsoluteContinuous(v.inverse()),
            ControlValue::Relative(v) => ControlValue::Relative(v.inverse()),
            ControlValue::AbsoluteDiscrete(v) => ControlValue::AbsoluteDiscrete(v.inverse()),
        }
    }

    pub fn to_absolute_continuous(self) -> Result<ControlValue, &'static str> {
        match self {
            ControlValue::AbsoluteContinuous(v) => Ok(ControlValue::AbsoluteContinuous(v)),
            ControlValue::Relative(_) => Err("relative value can't be normalized"),
            ControlValue::AbsoluteDiscrete(v) => Ok(ControlValue::AbsoluteContinuous(v.into())),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum AbsoluteValue {
    Continuous(UnitValue),
    Discrete(Fraction),
}

impl AbsoluteValue {
    // TODO-high Maybe remove
    pub fn to_unit_value(self) -> UnitValue {
        match self {
            AbsoluteValue::Continuous(v) => v,
            AbsoluteValue::Discrete(v) => v.to_unit_value(),
        }
    }
}

// TODO-high Remove!!!
impl Deref for AbsoluteValue {
    type Target = UnitValue;

    fn deref(&self) -> &Self::Target {
        match self {
            AbsoluteValue::Continuous(v) => &v,
            AbsoluteValue::Discrete(f) => &f.unit_value,
        }
    }
}

impl Default for AbsoluteValue {
    fn default() -> Self {
        Self::Continuous(Default::default())
    }
}
