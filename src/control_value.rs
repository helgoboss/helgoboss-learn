use crate::{DiscreteIncrement, UnitValue};

/// Value coming from a source (e.g. a MIDI source) which is supposed to control something.
#[derive(Clone, Copy, Debug, PartialEq)]
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
}
