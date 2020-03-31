use crate::{DiscreteIncrement, Interval, UnitValue};
use helgoboss_midi::SevenBitValue;
use std::ops::{Add, Div, Sub};

pub enum ControlValue {
    Absolute(UnitValue),
    Relative(DiscreteIncrement),
}
