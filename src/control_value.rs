use crate::{DiscreteIncrement, Interval, UnitValue};
use helgoboss_midi::SevenBitValue;
use std::ops::{Add, Div, Sub};

/// Value coming from a source (e.g. a MIDI source) which is supposed to control something.
pub enum ControlValue {
    /// Absolute value (e.g. fader position, knob position, key press, key release)
    Absolute(UnitValue),
    /// Relative value (e.g. encoder movement)
    Relative(DiscreteIncrement),
}
