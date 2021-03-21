use crate::{DiscreteIncrement, DiscreteValue, Interval};
use derive_more::Display;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::convert::{TryFrom, TryInto};
use std::fmt::Debug;
use std::ops::{Add, Sub};

/// A number that is primarily within the negative and positive unit interval `(-1.0..=1.0)` but
/// can also take higher values.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Display, Default)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(try_from = "f64")
)]
pub struct SoftSymmetricUnitValue(f64);

impl SoftSymmetricUnitValue {
    /// -1.0
    pub const SOFT_MIN: SoftSymmetricUnitValue = SoftSymmetricUnitValue(-1.0);

    /// 1.0
    pub const SOFT_MAX: SoftSymmetricUnitValue = SoftSymmetricUnitValue(1.0);

    /// Creates the symmetric unit value. Panics if the given number is not within the positive unit
    /// interval.
    pub fn new(number: f64) -> SoftSymmetricUnitValue {
        SoftSymmetricUnitValue(number)
    }

    /// Returns the underlying number.
    pub fn get(&self) -> f64 {
        self.0
    }

    pub fn abs(&self) -> UnitValue {
        UnitValue::new_clamped(self.0.abs())
    }

    pub fn map_to_positive_unit_interval(&self) -> UnitValue {
        UnitValue::new_clamped((self.0 + 1.0) / 2.0)
    }

    pub fn clamp_to_positive_unit_interval(&self) -> UnitValue {
        if self.0 < 0.0 {
            UnitValue::MIN
        } else {
            UnitValue::new_clamped(self.0)
        }
    }
}

impl Add for SoftSymmetricUnitValue {
    type Output = f64;

    fn add(self, rhs: Self) -> Self::Output {
        self.0 + rhs.0
    }
}

impl Sub for SoftSymmetricUnitValue {
    type Output = f64;

    fn sub(self, rhs: Self) -> Self::Output {
        self.0 - rhs.0
    }
}

impl From<f64> for SoftSymmetricUnitValue {
    fn from(v: f64) -> Self {
        SoftSymmetricUnitValue(v)
    }
}

impl std::str::FromStr for SoftSymmetricUnitValue {
    type Err = &'static str;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let primitive = f64::from_str(source).map_err(|_| "not a valid decimal number")?;
        Ok(SoftSymmetricUnitValue(primitive))
    }
}

/// Defines the normalization behavior if the range span is zero (that is min == max).
pub enum MinIsMaxBehavior {
    PreferZero,
    PreferOne,
}

/// A number within the unit interval `(0.0..=1.0)`.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Display, Default)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(try_from = "f64")
)]
pub struct UnitValue(f64);

impl UnitValue {
    /// 0.0
    pub const MIN: UnitValue = UnitValue(0.0);

    /// 1.0
    pub const MAX: UnitValue = UnitValue(1.0);

    pub fn is_valid(number: f64) -> bool {
        (0.0..=1.0).contains(&number)
    }

    /// Creates the unit value. Panics if the given number is not within the positive unit interval.
    pub fn new(number: f64) -> UnitValue {
        assert!(
            Self::is_valid(number),
            format!("{} is not a valid unit value", number)
        );
        UnitValue(number)
    }

    pub fn new_clamped(number: f64) -> UnitValue {
        let actual_number = if number > 1.0 {
            1.0
        } else if number < 0.0 {
            0.0
        } else {
            number
        };
        UnitValue(actual_number)
    }

    /// Checks preconditions only in debug build. Should only be used if you want to squeeze out
    /// every last bit of performance and you are super sure that the number meets the
    /// preconditions. This constructor is offered because it's not unlikely that a lot of those
    /// values will be constructed in audio thread.
    ///
    /// # Safety
    ///
    /// You need to make sure that the given number is a valid unit value.
    pub unsafe fn new_unchecked(number: f64) -> UnitValue {
        debug_assert!(
            Self::is_valid(number),
            format!("{} is not a valid unit value", number)
        );
        UnitValue(number)
    }

    // TODO Maybe we should rather implement From<UnitValue> for f64? Same with other newtypes.
    /// Returns the underlying number.
    pub fn get(&self) -> f64 {
        self.0
    }

    pub fn to_symmetric(&self) -> SoftSymmetricUnitValue {
        SoftSymmetricUnitValue::new(self.0)
    }

    pub fn map_to_symmetric_unit_interval(&self) -> SoftSymmetricUnitValue {
        SoftSymmetricUnitValue::new((self.0 * 2.0) - 1.0)
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
        let min = destination_interval.min_val().get();
        let span = destination_interval.span();
        unsafe { UnitValue::new_unchecked(min + self.get() * span) }
    }

    /// Maps this value to the unit interval assuming that this value currently exhausts the given
    /// current interval. If this value is outside the current interval, this method returns either
    /// 0.0 or 1.0. If value == min == max, it returns 1.0.
    pub fn map_to_unit_interval_from(
        &self,
        current_interval: &Interval<UnitValue>,
        min_is_max_behavior: MinIsMaxBehavior,
    ) -> UnitValue {
        let (min, max) = (current_interval.min_val(), current_interval.max_val());
        if *self < min {
            return UnitValue::MIN;
        }
        if *self > max {
            return UnitValue::MAX;
        }
        if min == max {
            use MinIsMaxBehavior::*;
            return match min_is_max_behavior {
                PreferZero => UnitValue::MIN,
                PreferOne => UnitValue::MAX,
            };
        }
        unsafe { UnitValue::new_unchecked((*self - min) / current_interval.span()) }
    }

    /// Like `map_from_unit_interval_to` but mapping to a discrete range (with additional rounding).
    /// round() is used here instead of floor() in order to not give advantage to any direction.
    pub fn map_from_unit_interval_to_discrete(
        &self,
        destination_interval: &Interval<DiscreteValue>,
    ) -> DiscreteValue {
        let min = destination_interval.min_val().get();
        let span = destination_interval.span();
        DiscreteValue::new(min + (self.get() * span as f64).round() as u32)
    }

    pub fn map_from_unit_interval_to_discrete_increment(
        &self,
        destination_interval: &Interval<DiscreteIncrement>,
    ) -> DiscreteIncrement {
        let min: i32 = destination_interval.min_val().get();
        let max: i32 = destination_interval.max_val().get();
        let count: u32 = if min < 0 && max > 0 {
            (max - min) as u32
        } else {
            (max - min) as u32 + 1
        };
        let addend: u32 = (self.0 * (count - 1) as f64).round() as _;
        let sum = min + addend as i32;
        let skip_zero_sum = if min < 0 && sum >= 0 { sum + 1 } else { sum };
        DiscreteIncrement::new(skip_zero_sum)
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
        assert_ne!(interval_count, 0);
        let interval_count = interval_count as f64;
        unsafe { UnitValue::new_unchecked((self.0 * interval_count).round() / interval_count) }
    }

    // Rounds value to its nearest grid value using the grid's interval size. If you pass an
    // interval size whose multiple doesn't perfectly fit into the unit interval, the last
    // interval will be smaller than all the others. Better don't do that.
    pub fn snap_to_grid_by_interval_size(&self, interval_size: UnitValue) -> UnitValue {
        if interval_size.is_zero() {
            return *self;
        }
        unsafe {
            UnitValue::new_unchecked(
                ((self.0 / interval_size.0).round() * interval_size.0).min(1.0),
            )
        }
    }

    /// Returns whether this is exactly 0.0.
    #[allow(clippy::float_cmp)]
    pub fn is_zero(&self) -> bool {
        self.0 == 0.0
    }

    /// Returns whether this is exactly 1.0.
    #[allow(clippy::float_cmp)]
    pub fn is_one(&self) -> bool {
        self.0 == 1.0
    }

    /// Adds the given increment. If the result doesn't fit into the given interval anymore, it just
    /// snaps to the opposite bound of that interval. If this unit value is not within the given
    /// interval in the first place, it returns an appropriate interval bound instead of doing the
    /// addition.
    ///
    /// Slight inaccuracies can have a big effect when actually rotating:
    /// https://github.com/helgoboss/realearn/issues/208. That's why an epsilon needs to be passed
    /// for the comparison that decides whether it's time to rotate already.
    pub fn add_rotating(
        &self,
        increment: UnitIncrement,
        interval: &Interval<UnitValue>,
        epsilon: f64,
    ) -> UnitValue {
        let (min, max) = (interval.min_val(), interval.max_val());
        if *self < min {
            return if increment.is_positive() { min } else { max };
        }
        if *self > max {
            return if increment.is_positive() { min } else { max };
        }
        let sum = self.0 + increment.get();
        if sum < min.get() {
            if (min.get() - sum).abs() <= epsilon {
                min
            } else {
                max
            }
        } else if sum > max.get() {
            if (sum - max.get()).abs() <= epsilon {
                max
            } else {
                min
            }
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
        let (min, max) = (interval.min_val(), interval.max_val());
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
        unsafe {
            UnitValue::new_unchecked(num::clamp(
                self.0,
                interval.min_val().0,
                interval.max_val().0,
            ))
        }
    }

    pub fn to_discrete<T: TryFrom<u64> + Into<u64>>(&self, max_value: T) -> T
    where
        <T as TryFrom<u64>>::Error: Debug,
    {
        let discrete = (self.get() * max_value.into() as f64).round() as u64;
        discrete.try_into().unwrap()
    }

    pub fn try_from_discrete<T: TryFrom<u64> + Into<u64>>(
        actual_value: T,
        max_value: T,
    ) -> Result<UnitValue, &'static str> {
        let actual_value = actual_value.into();
        let max_value = max_value.into();
        if actual_value > max_value {
            return Err("value too large");
        }
        let unit_value = Self::new_clamped(actual_value as f64 / max_value as f64);
        Ok(unit_value)
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

impl TryFrom<f64> for UnitValue {
    type Error = &'static str;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if !UnitValue::is_valid(value) {
            return Err("value is not between 0.0 and 1.0");
        }
        Ok(UnitValue(value))
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
        unsafe { UnitValue::new_unchecked((self.min_val() + self.max_val()) / 2.0) }
    }

    /// Returns whether this interval is the complete unit interval.
    pub fn is_full(&self) -> bool {
        self.min_val().is_zero() && self.max_val().is_one()
    }

    /// Inverts the interval.
    pub fn inverse(&self) -> Interval<UnitValue> {
        Interval::new(self.max_val().inverse(), self.min_val().inverse())
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
    #[allow(clippy::float_cmp)]
    pub fn new(increment: f64) -> UnitIncrement {
        assert_ne!(increment, 0.0);
        UnitIncrement(increment)
    }

    /// Checks preconditions only in debug build. Should only be used if you want to squeeze out
    /// every last bit of performance and you are super sure that the number meets the
    /// preconditions. This constructor is offered because it's not unlikely that a lot of those
    /// values will be constructed in audio thread.
    ///
    /// # Safety
    ///
    /// Make sure the given increment is not zero.
    #[allow(clippy::float_cmp)]
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
    pub fn clamp_to_interval(&self, interval: &Interval<UnitValue>) -> Option<UnitIncrement> {
        let clamped_value = self.to_value().clamp_to_interval(interval);
        clamped_value.to_increment(self.signum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_from_unit_interval_to_discrete_increment() {
        // Given
        // Contains elements -3, -2, -1, 1, 2, 3, 4
        let interval = Interval::new(DiscreteIncrement::new(-3), DiscreteIncrement::new(4));
        // When
        // Then
        assert_eq!(
            UnitValue::new(0.0).map_from_unit_interval_to_discrete_increment(&interval),
            DiscreteIncrement::new(-3)
        );
        assert_eq!(
            UnitValue::new(0.5).map_from_unit_interval_to_discrete_increment(&interval),
            DiscreteIncrement::new(1)
        );
        assert_eq!(
            UnitValue::new(1.0).map_from_unit_interval_to_discrete_increment(&interval),
            DiscreteIncrement::new(4)
        );
    }
}
