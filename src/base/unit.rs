use crate::{DiscreteValue, Interval};
use derive_more::Display;
use std::ops::{Add, Sub};

/// A number within the unit interval `(0.0..=1.0)`.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Display)]
pub struct UnitValue(f64);

impl UnitValue {
    /// 0.0
    pub const MIN: UnitValue = UnitValue(0.0);

    /// 1.0
    pub const MAX: UnitValue = UnitValue(1.0);

    pub fn is_valid(number: f64) -> bool {
        0.0 <= number && number <= 1.0
    }

    /// Creates the unit value. Panics if the given number is not within the positive unit interval.
    pub fn new(number: f64) -> UnitValue {
        assert!(UnitValue::is_valid(number));
        UnitValue(number)
    }

    /// Checks preconditions only in debug build. Should only be used if you want to squeeze out
    /// every last bit of performance and you are super sure that the number meets the
    /// preconditions. This constructor is offered because it's not unlikely that a lot of those
    /// values will be constructed in audio thread.
    pub unsafe fn new_unchecked(number: f64) -> UnitValue {
        debug_assert!(0.0 <= number && number <= 1.0);
        UnitValue(number)
    }

    // TODO Maybe we should rather implement From<UnitValue> for f64? Same with other newtypes.
    /// Returns the underlying number.
    pub fn get(&self) -> f64 {
        self.0
    }

    /// Tests if this value is within the given interval.
    pub fn is_within_interval(&self, interval: &Interval<UnitValue>) -> bool {
        interval.contains(*self)
    }

    /// Calculates the distance between this and another unit value.
    pub fn calc_distance_from(&self, rhs: Self) -> UnitValue {
        unsafe { UnitValue::new_unchecked((self.0 - rhs.0).abs()) }
    }

    /// Maps this value to the given destination interval assuming that this value currently
    /// exhausts the complete unit interval.
    pub fn map_from_unit_interval_to(
        &self,
        destination_interval: &Interval<UnitValue>,
    ) -> UnitValue {
        let min = destination_interval.min().get();
        let span = destination_interval.span();
        unsafe { UnitValue::new_unchecked(min + self.get() * span) }
    }

    /// Maps this value to the unit interval assuming that this value currently exhausts the given
    /// source interval. If this value is outside the source interval, this method returns either
    /// 0.0 or 1.0.
    pub fn map_to_unit_interval_from(&self, source_interval: &Interval<UnitValue>) -> UnitValue {
        let (min, max) = (source_interval.min(), source_interval.max());
        if *self < min {
            return UnitValue::MIN;
        }
        if *self > max {
            return UnitValue::MAX;
        }
        unsafe { UnitValue::new_unchecked((*self - min) / source_interval.span()) }
    }

    /// Like `map_from_unit_interval_to` but mapping to a discrete range (with additional rounding).
    /// round() is used here instead of floor() in order to not give advantage to any direction.
    pub fn map_from_unit_interval_to_discrete(
        &self,
        destination_interval: &Interval<DiscreteValue>,
    ) -> DiscreteValue {
        let min = destination_interval.min().get();
        let span = destination_interval.span();
        DiscreteValue::new(min + (self.get() * span as f64).round() as u32)
    }

    /// Converts this unit value to a unit increment, either negative or positive depending
    /// on the given signum. Returns `None` if this value is zero.
    pub fn to_increment(&self, signum: i32) -> Option<UnitIncrement> {
        if self.is_zero() {
            return None;
        }
        Some(unsafe { UnitIncrement::new_unchecked(signum as f64 * self.0) })
    }

    /// Returns the value on the "other side" of the unit interval.
    ///
    /// # Examples
    /// - 0.8 => 0.2
    /// - 0.6 => 0.4
    pub fn inverse(&self) -> UnitValue {
        unsafe { UnitValue::new_unchecked(1.0 - self.0) }
    }

    /// "Rounds" value to its nearest grid value using the grid's number of intervals. Using the
    /// number of intervals guarantees that each grid interval will have the same size. So if you
    /// have the accurate number of intervals at disposal, use this method.
    pub fn snap_to_grid_by_interval_count(&self, interval_count: u32) -> UnitValue {
        let interval_count = interval_count as f64;
        unsafe { UnitValue::new_unchecked((self.0 * interval_count).round() / interval_count) }
    }

    // Rounds value to its nearest grid value using the grid's interval size. If you pass an
    // interval size whose multiple doesn't perfectly fit into the unit interval, the last
    // interval will be smaller than all the others. Better don't do that.
    pub fn snap_to_grid_by_interval_size(&self, interval_size: UnitValue) -> UnitValue {
        unsafe { UnitValue::new_unchecked((self.0 / interval_size.0).round() * interval_size.0) }
    }

    /// Returns whether this is 0.0.
    pub fn is_zero(&self) -> bool {
        self.0 == 0.0
    }

    /// Returns whether this is 1.0.
    pub fn is_one(&self) -> bool {
        self.0 == 1.0
    }

    /// Adds the given increment. If the result doesn't fit into the given interval anymore, it just
    /// snaps to the opposite bound of that interval. If this unit value is not within the given
    /// interval in the first place, it returns an appropriate interval bound instead of doing the
    /// addition.
    pub fn add_rotating(
        &self,
        increment: UnitIncrement,
        interval: &Interval<UnitValue>,
    ) -> UnitValue {
        let (min, max) = (interval.min(), interval.max());
        if *self < min {
            return if increment.is_positive() { min } else { max };
        }
        if *self > max {
            return if increment.is_positive() { min } else { max };
        }
        let sum = self.0 + increment.get();
        if sum < min.get() {
            max
        } else if sum > max.get() {
            min
        } else {
            unsafe { UnitValue::new_unchecked(sum) }
        }
    }

    /// Adds the given increment. If the result doesn't fit into the given interval anymore, it just
    /// snaps to the bound of that interval. If this unit value is not within the given interval in
    /// the first place, it returns the closest interval bound instead of doing the addition.
    pub fn add_clamping(
        &self,
        increment: UnitIncrement,
        interval: &Interval<UnitValue>,
    ) -> UnitValue {
        let (min, max) = (interval.min(), interval.max());
        if *self < min {
            return min;
        }
        if *self > max {
            return max;
        }
        unsafe {
            UnitValue::new_unchecked(num::clamp(self.0 + increment.get(), min.get(), max.get()))
        }
    }

    /// Clamps this value to the given interval bounds.
    pub fn clamp_to_interval(&self, interval: &Interval<UnitValue>) -> UnitValue {
        unsafe { UnitValue::new_unchecked(num::clamp(self.0, interval.min().0, interval.max().0)) }
    }
}

impl Add for UnitValue {
    type Output = f64;

    fn add(self, rhs: Self) -> Self::Output {
        self.0 + rhs.0
    }
}

impl Sub for UnitValue {
    type Output = f64;

    fn sub(self, rhs: Self) -> Self::Output {
        self.0 - rhs.0
    }
}

impl std::str::FromStr for UnitValue {
    type Err = &'static str;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let primitive = f64::from_str(source).map_err(|_| "not a valid decimal number")?;
        if !UnitValue::is_valid(primitive) {
            return Err("not a value between 0.0 and 1.0");
        }
        Ok(UnitValue(primitive))
    }
}

impl Interval<UnitValue> {
    /// Returns the value which is exactly in the middle between the interval bounds.
    pub fn center(&self) -> UnitValue {
        unsafe { UnitValue::new_unchecked((self.min() + self.max()) / 2.0) }
    }

    /// Returns whether this interval is the complete unit interval.
    pub fn is_full(&self) -> bool {
        self.min().is_zero() && self.max().is_one()
    }
}

/// Convenience method for getting the complete unit interval.
pub fn full_unit_interval() -> Interval<UnitValue> {
    create_unit_value_interval(0.0, 1.0)
}

/// Convenience method for creating an interval of unit values.
pub fn create_unit_value_interval(min: f64, max: f64) -> Interval<UnitValue> {
    Interval::new(UnitValue::new(min), UnitValue::new(max))
}

/// A number within the negative or positive unit interval `(-1.0..=1.0)` representing a positive or
/// negative increment, never 0 (otherwise it wouldn't be an increment after all).
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct UnitIncrement(f64);

impl UnitIncrement {
    /// Creates the unit increment. Panics if the given number is 0.0.
    pub fn new(increment: f64) -> UnitIncrement {
        assert_ne!(increment, 0.0);
        UnitIncrement(increment)
    }

    /// Checks preconditions only in debug build. Should only be used if you want to squeeze out
    /// every last bit of performance and you are super sure that the number meets the
    /// preconditions. This constructor is offered because it's not unlikely that a lot of those
    /// values will be constructed in audio thread.
    pub unsafe fn new_unchecked(increment: f64) -> UnitIncrement {
        debug_assert_ne!(increment, 0.0);
        UnitIncrement(increment)
    }

    /// Returns the underlying number.
    pub fn get(&self) -> f64 {
        self.0
    }

    /// Returns if this increment is positive.
    pub fn is_positive(&self) -> bool {
        self.0 >= 0.0
    }

    /// Returns the signum (-1 if it's a negative increment, otherwise +1).
    pub fn signum(&self) -> i32 {
        if self.is_positive() { 1 } else { -1 }
    }

    /// Converts this unit increment into a unit value thereby "losing" its direction.
    pub fn to_value(&self) -> UnitValue {
        unsafe { UnitValue::new_unchecked(self.0.abs()) }
    }

    /// Clamps this increment to the given interval bounds.
    pub fn clamp_to_interval(&self, interval: &Interval<UnitValue>) -> UnitIncrement {
        let clamped_value = self.to_value().clamp_to_interval(interval);
        clamped_value.to_increment(self.signum()).unwrap()
    }
}
