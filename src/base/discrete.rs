use crate::{Interval, UnitIncrement, UnitValue};
use derive_more::Display;
use helgoboss_midi::U7;
use std::cmp;
use std::convert::TryFrom;
use std::ops::Sub;

/// A positive discrete number most likely representing a step count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Display)]
pub struct DiscreteValue(u32);

impl DiscreteValue {
    /// Creates the discrete value.
    pub const fn new(value: u32) -> DiscreteValue {
        DiscreteValue(value)
    }

    /// Returns the underlying number.
    pub fn get(&self) -> u32 {
        self.0
    }

    /// Converts this discrete value to a discrete increment, either negative or positive depending
    /// on the given signum. Returns `None` if this value is zero.
    pub fn to_increment(self, signum: i32) -> Option<DiscreteIncrement> {
        if self.is_zero() {
            return None;
        }
        Some(unsafe { DiscreteIncrement::new_unchecked(signum * self.0 as i32) })
    }

    /// Returns whether this is 0.
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Clamps this value to the given interval bounds.
    pub fn clamp_to_interval(&self, interval: &Interval<DiscreteValue>) -> DiscreteValue {
        DiscreteValue::new(num::clamp(
            self.0,
            interval.min_val().0,
            interval.max_val().0,
        ))
    }
}

impl std::str::FromStr for DiscreteValue {
    type Err = &'static str;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let primitive = u32::from_str(source).map_err(|_| "not a valid positive integer")?;
        Ok(DiscreteValue(primitive))
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
        assert_ne!(increment, 0);
        DiscreteIncrement(increment)
    }

    /// Checks preconditions only in debug build. Should only be used if you want to squeeze out
    /// every last bit of performance and you are super sure that the number meets the
    /// preconditions. This constructor is offered because it's not unlikely that a lot of those
    /// values will be constructed in audio thread.
    ///
    /// # Safety
    ///
    /// Make sure the given increment is not zero.
    pub unsafe fn new_unchecked(increment: i32) -> DiscreteIncrement {
        debug_assert_ne!(increment, 0);
        DiscreteIncrement(increment)
    }

    /// Creates an increment from the given MIDI control-change value assuming that the device
    /// emitting the control-change messages uses a protocol which is called "Relative 1" in REAPER.
    ///
    /// - 127 = decrement; 0 = none; 1 = increment
    /// - 127 > value > 63 results in higher decrement step sizes (64 possible decrement step sizes)
    /// - 1 < value <= 63 results in higher increment step sizes (63 possible increment step sizes)
    pub fn from_encoder_1_value(value: U7) -> Result<DiscreteIncrement, &'static str> {
        let value = value.get();
        if value == 0 {
            return Err("increment must not be zero");
        }
        let increment = if value <= 63 {
            // Zero and increment
            value as i32
        } else {
            // Decrement
            -((128 - value) as i32)
        };
        Ok(unsafe { DiscreteIncrement::new_unchecked(increment) })
    }

    /// Creates an increment from the given MIDI control-change value assuming that the device
    /// emitting the control-change messages uses a protocol which is called "Relative 2" in REAPER.
    ///
    /// - 63 = decrement; 64 = none; 65 = increment
    /// - 63 > value >= 0 results in higher decrement step sizes (64 possible decrement step sizes)
    /// - 65 < value <= 127 results in higher increment step sizes (63 possible increment step
    ///   sizes)
    pub fn from_encoder_2_value(value: U7) -> Result<DiscreteIncrement, &'static str> {
        let value = value.get();
        if value == 64 {
            return Err("increment must not be zero");
        }
        let increment = if value > 64 {
            // Zero and increment
            (value - 64) as i32
        } else {
            // Decrement
            -((64 - value) as i32)
        };
        Ok(unsafe { DiscreteIncrement::new_unchecked(increment) })
    }

    /// Creates an increment from the given MIDI control-change value assuming that the device
    /// emitting the control-change messages uses a protocol which is called "Relative 3" in REAPER.
    ///
    /// - 65 = decrement; 0 = none; 1 = increment
    /// - 65 < value <= 127 results in higher decrement step sizes (63 possible decrement step
    ///   sizes)
    /// - 1 < value <= 64 results in higher increment step sizes (64 possible increment step sizes)
    pub fn from_encoder_3_value(value: U7) -> Result<DiscreteIncrement, &'static str> {
        let value = value.get();
        if value == 0 {
            return Err("increment must not be zero");
        }
        let increment = if value <= 64 {
            // Zero and increment
            value as i32
        } else {
            // Decrement
            -((value - 64) as i32)
        };
        Ok(unsafe { DiscreteIncrement::new_unchecked(increment) })
    }

    /// Clamps this increment to the given interval bounds.
    pub fn clamp_to_interval(&self, interval: &Interval<DiscreteIncrement>) -> DiscreteIncrement {
        // Step count interval: (-3, 4) = -3, -2, -1, 1, 2, 3, 4
        // 1 => -3
        // 2 => -2
        // 7 =>  4
        // 8 =>  4
        // Step count interval: (4, 10) = 4, 5, 6, 7, 8, 9, 10
        // 1 => 4
        // 2 => 5
        // 7 => 10
        // 8 => 10
        let positive_increment = self.0.abs() as u32;
        let min: i32 = interval.min_val().get();
        let max: i32 = interval.max_val().get();
        let count: u32 = if min < 0 && max > 0 {
            (max - min) as u32
        } else {
            (max - min) as u32 + 1
        };
        let addend: u32 = cmp::min(positive_increment - 1, count - 1);
        let sum = min + addend as i32;
        let skip_zero_sum = if min < 0 && sum >= 0 { sum + 1 } else { sum };
        let clamped = cmp::min(skip_zero_sum, max);
        DiscreteIncrement::new(clamped)
    }

    /// Converts this discrete increment into a discrete value thereby "losing" its direction.
    pub fn to_value(self) -> DiscreteValue {
        DiscreteValue::new(self.0.abs() as u32)
    }

    /// Switches the direction of this increment (makes a positive one negative and vice versa).
    pub fn inverse(&self) -> DiscreteIncrement {
        unsafe { DiscreteIncrement::new_unchecked(-self.0) }
    }

    pub fn with_direction(&self, signum: i32) -> DiscreteIncrement {
        let abs = self.0.abs();
        let inner = if signum >= 0 { abs } else { -abs };
        DiscreteIncrement::new(inner)
    }

    /// Returns the underlying number.
    pub fn get(&self) -> i32 {
        self.0
    }

    /// Returns if this increment is positive.
    pub fn is_positive(&self) -> bool {
        self.0 >= 0
    }

    /// Returns the signum (-1 if it's a negative increment, otherwise +1).
    pub fn signum(&self) -> i32 {
        if self.is_positive() {
            1
        } else {
            -1
        }
    }

    /// Returns a unit increment or None in case of 0.0. The unit increment is built by creating a
    /// multiple of the given atomic unit value (= minimum step size) and clamping the result if it
    /// exceeds the unit interval.
    pub fn to_unit_increment(self, atomic_unit_value: UnitValue) -> Option<UnitIncrement> {
        let positive_large = self.to_value().get() as f64 * atomic_unit_value.get();
        let unit_value = UnitValue::new(num::clamp(positive_large, 0.0, 1.0));
        unit_value.to_increment(self.signum())
    }
}

impl Sub for DiscreteIncrement {
    type Output = i32;

    fn sub(self, rhs: Self) -> Self::Output {
        self.0 - rhs.0
    }
}

impl TryFrom<i32> for DiscreteIncrement {
    type Error = &'static str;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        if value == 0 {
            return Err("zero is not an increment");
        }
        Ok(DiscreteIncrement::new(value))
    }
}

/// Convenience method for creating an interval of discrete increments.
pub fn create_discrete_increment_interval(min: i32, max: i32) -> Interval<DiscreteIncrement> {
    Interval::new(DiscreteIncrement::new(min), DiscreteIncrement::new(max))
}
