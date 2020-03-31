use crate::{Interval, UnitIncrement, UnitValue};
use helgoboss_midi::{SevenBitValue, SEVEN_BIT_VALUE_MAX};
use std::ops::Sub;

/// A positive discrete number representing a step count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiscreteValue(u32);

impl DiscreteValue {
    /// Creates the discrete value.
    pub fn new(value: u32) -> DiscreteValue {
        DiscreteValue(value)
    }

    /// Returns the underlying number.
    pub fn get_number(&self) -> u32 {
        self.0
    }

    /// Converts this discrete value to a discrete increment, either negative or positive depending
    /// on the given signum. Returns `None` if this value is zero.
    pub fn to_increment(&self, signum: i32) -> Option<DiscreteIncrement> {
        if self.is_zero() {
            return None;
        }
        Some(DiscreteIncrement::new(signum * self.0 as i32))
    }

    /// Returns whether this is 0.
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Clamps this value to the given interval bounds.
    pub fn clamp_to_interval(&self, interval: &Interval<DiscreteValue>) -> DiscreteValue {
        DiscreteValue::new(self.0.clamp(interval.get_min().0, interval.get_max().0))
    }
}

impl Sub for DiscreteValue {
    type Output = u32;

    fn sub(self, rhs: Self) -> Self::Output {
        self.0 - rhs.0
    }
}

/// A discrete number representing a positive or negative increment, never 0 (otherwise it wouldn't
/// be an increment after all).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiscreteIncrement(i32);

impl DiscreteIncrement {
    /// Creates the discrete increment. Panics if the given number is 0.
    pub fn new(increment: i32) -> DiscreteIncrement {
        debug_assert_ne!(increment, 0);
        DiscreteIncrement(increment)
    }

    /// Creates an increment from the given MIDI control-change value assuming that the device
    /// emitting the control-change messages uses a protocol which is called "Relative 1" in REAPER.
    ///
    /// - 127 = decrement; 0 = none; 1 = increment
    /// - 127 > value > 63 results in higher decrement step sizes (64 possible decrement step sizes)
    /// - 1 < value <= 63 results in higher increment step sizes (63 possible increment step sizes)
    pub fn from_encoder_1_value(value: SevenBitValue) -> Result<DiscreteIncrement, ()> {
        debug_assert!(value <= SEVEN_BIT_VALUE_MAX);
        if value == 0 {
            return Err(());
        }
        let increment = if value <= 63 {
            // Zero and increment
            value as i32
        } else {
            // Decrement
            -1 * (128 - value) as i32
        };
        Ok(DiscreteIncrement::new(increment))
    }

    /// Creates an increment from the given MIDI control-change value assuming that the device
    /// emitting the control-change messages uses a protocol which is called "Relative 2" in REAPER.
    ///
    /// - 63 = decrement; 64 = none; 65 = increment
    /// - 63 > value >= 0 results in higher decrement step sizes (64 possible decrement step sizes)
    /// - 65 < value <= 127 results in higher increment step sizes (63 possible increment step
    ///   sizes)
    pub fn from_encoder_2_value(value: SevenBitValue) -> Result<DiscreteIncrement, ()> {
        debug_assert!(value <= SEVEN_BIT_VALUE_MAX);
        if value == 64 {
            return Err(());
        }
        let increment = if value >= 64 {
            // Zero and increment
            (value - 64) as i32
        } else {
            // Decrement
            -1 * (64 - value) as i32
        };
        Ok(DiscreteIncrement::new(increment))
    }

    /// Creates an increment from the given MIDI control-change value assuming that the device
    /// emitting the control-change messages uses a protocol which is called "Relative 3" in REAPER.
    ///
    /// - 65 = decrement; 0 = none; 1 = increment
    /// - 65 < value <= 127 results in higher decrement step sizes (63 possible decrement step
    ///   sizes)
    /// - 1 < value <= 64 results in higher increment step sizes (64 possible increment step sizes)
    pub fn from_encoder_3_value(value: SevenBitValue) -> Result<DiscreteIncrement, ()> {
        debug_assert!(value <= SEVEN_BIT_VALUE_MAX);
        if value == 0 {
            return Err(());
        }
        let increment = if value <= 64 {
            // Zero and increment
            value as i32
        } else {
            // Decrement
            -1 * (value - 64) as i32
        };
        Ok(DiscreteIncrement::new(increment))
    }

    /// Clamps this increment to the given interval bounds.
    pub fn clamp_to_interval(&self, interval: &Interval<DiscreteValue>) -> DiscreteIncrement {
        let clamped_value = self.to_value().clamp_to_interval(interval);
        clamped_value.to_increment(self.get_signum()).unwrap()
    }

    /// Converts this discrete increment into a discrete value thereby "losing" its direction.
    pub fn to_value(&self) -> DiscreteValue {
        DiscreteValue::new(self.0.abs() as u32)
    }

    /// Switches the direction of this increment (makes a positive one negative and vice versa).
    pub fn inverse(&self) -> DiscreteIncrement {
        DiscreteIncrement::new(-self.0)
    }

    /// Returns the underlying number.
    pub fn get_number(&self) -> i32 {
        self.0
    }

    /// Returns if this increment is positive.
    pub fn is_positive(&self) -> bool {
        self.0 >= 0
    }

    /// Returns the signum (-1 if it's a negative increment, otherwise +1).
    pub fn get_signum(&self) -> i32 {
        if self.is_positive() { 1 } else { -1 }
    }

    /// Returns a unit increment or None in case of 0.0. The unit increment is built by creating a
    /// multiple of the given atomic unit value (= minimum step size) and clamping the result if it
    /// exceeds the unit interval.
    pub fn to_unit_increment(&self, atomic_unit_value: UnitValue) -> Option<UnitIncrement> {
        let positive_large = self.to_value().get_number() as f64 * atomic_unit_value.get_number();
        let unit_value = UnitValue::new(positive_large.clamp(0.0, 1.0));
        unit_value.to_increment(self.get_signum())
    }
}

impl Sub for DiscreteIncrement {
    type Output = i32;

    fn sub(self, rhs: Self) -> Self::Output {
        self.0 - rhs.0
    }
}
