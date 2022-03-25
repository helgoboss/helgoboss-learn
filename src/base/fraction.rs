use crate::{DiscreteIncrement, Interval, IntervalMatchResult, MinIsMaxBehavior, UnitValue};
use std::fmt::{Display, Formatter};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Fraction {
    /// Concrete discrete value.
    actual: u32,
    /// Soft maximum value: Good to know in order to be able to instantly convert to a UnitValue
    /// whenever we want to go absolute-continuous.
    max: u32,
}

impl Display for Fraction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.actual, self.max)
    }
}

impl Fraction {
    pub const MIN: Fraction = Fraction::new_max(0);

    pub const fn new(actual: u32, max: u32) -> Self {
        Self { actual, max }
    }

    pub const fn new_min(max: u32) -> Self {
        Self::new(0, max)
    }

    pub const fn new_max(max: u32) -> Self {
        Self::new(max, max)
    }

    pub const fn actual(&self) -> u32 {
        self.actual
    }

    pub fn actual_clamped(&self) -> u32 {
        std::cmp::min(self.actual, self.max)
    }

    pub const fn max_val(&self) -> u32 {
        self.max
    }

    pub fn with_actual(&self, actual: u32) -> Self {
        Self::new(actual, self.max)
    }

    pub fn with_max(&self, max: u32) -> Self {
        Self::new(self.actual, max)
    }

    pub fn with_max_clamped(&self, max: u32) -> Self {
        Self::new(std::cmp::min(self.actual, max), max)
    }

    pub fn inverse(&self) -> Self {
        Self {
            actual: self.max - self.actual_clamped(),
            max: self.max,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.actual == 0
    }

    pub fn to_unit_value(self) -> UnitValue {
        if self.max == 0 {
            return UnitValue::MIN;
        }
        UnitValue::new(std::cmp::min(self.actual, self.max) as f64 / self.max as f64)
    }

    /// Tests if this value is within the given interval.
    pub fn is_within_interval(&self, interval: &Interval<u32>) -> bool {
        use IntervalMatchResult::*;
        match interval.value_matches(self.actual) {
            Between | Min | Max | MinAndMax => true,
            Lower | Greater => false,
        }
    }

    /// This value is supposed to be in the given interval.
    pub fn normalize(
        &self,
        interval: &Interval<u32>,
        min_is_max_behavior: MinIsMaxBehavior,
    ) -> Self {
        let rooted_max = {
            let unrooted_max = self.max;
            let min_span = unrooted_max - interval.min_val();
            std::cmp::min(min_span, interval.span())
        };
        use IntervalMatchResult::*;
        match interval.value_matches(self.actual) {
            Between => {
                let unrooted_actual = self.actual;
                // actual
                let rooted_actual = unrooted_actual - interval.min_val();
                // fraction
                Fraction::new(rooted_actual, rooted_max)
            }
            MinAndMax => {
                use MinIsMaxBehavior::*;
                match min_is_max_behavior {
                    PreferZero => Self::new_min(0),
                    PreferOne => Self::new_max(1),
                }
            }
            Min | Lower => Fraction::new_min(rooted_max),
            Max | Greater => Fraction::new_max(rooted_max),
        }
    }

    /// This value is supposed to be normalized (0-rooted).
    pub fn denormalize(&self, interval: &Interval<u32>, discrete_max: Option<u32>) -> Self {
        let new_max = discrete_max.unwrap_or(self.max);
        let clamped_interval_max = std::cmp::min(interval.max_val(), new_max);
        let denorm_actual = std::cmp::min(interval.min_val() + self.actual, clamped_interval_max);
        Fraction::new(denorm_actual, new_max)
    }

    /// Adds the given increment. If the result doesn't fit into the given interval anymore, it just
    /// snaps to the opposite bound of that interval. If this fraction is not within the given
    /// interval in the first place, it returns an appropriate interval bound instead of doing the
    /// addition.
    pub fn add_rotating(&self, increment: DiscreteIncrement, interval: &Interval<u32>) -> Fraction {
        let (min, max) = (interval.min_val(), interval.max_val());
        use IntervalMatchResult::*;
        let new_actual = match interval.value_matches(self.actual) {
            Lower | Greater => {
                if increment.is_positive() {
                    min
                } else {
                    max
                }
            }
            Between | Min | Max | MinAndMax => {
                let sum = self.actual as i32 + increment.get();
                if sum < 0 {
                    max
                } else {
                    let sum = sum as u32;
                    match interval.value_matches(sum) {
                        Between => sum,
                        Min | Greater => min,
                        Max | Lower | MinAndMax => max,
                    }
                }
            }
        };
        Fraction::new(new_actual, max)
    }

    /// Adds the given increment. If the result doesn't fit into the given interval anymore, it just
    /// snaps to the bound of that interval. If this fraction is not within the given interval in
    /// the first place, it returns the closest interval bound instead of doing the addition.
    pub fn add_clamping(&self, increment: DiscreteIncrement, interval: &Interval<u32>) -> Fraction {
        let (min, max) = (interval.min_val(), interval.max_val());
        use IntervalMatchResult::*;
        let new_actual = match interval.value_matches(self.actual) {
            Lower => min,
            Greater => max,
            Between | Min | Max | MinAndMax => {
                let sum = self.actual as i32 + increment.get();
                if sum < 0 {
                    min
                } else {
                    let sum = sum as u32;
                    match interval.value_matches(sum) {
                        Between => sum,
                        Min | Lower => min,
                        Max | Greater | MinAndMax => max,
                    }
                }
            }
        };
        Fraction::new(new_actual, max)
    }
}

impl Interval<u32> {
    pub fn normalize_to_min(&self, value: u32) -> u32 {
        let value = std::cmp::min(value, self.max_val());
        let difference = value as i32 - self.min_val() as i32;
        std::cmp::max(difference, 0) as u32
    }
}

pub fn full_discrete_interval() -> Interval<u32> {
    Interval::new(0, u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_subset() {
        // Given
        let source_interval = Interval::new(100, 120);
        // When
        // Then
        assert_eq!(
            Fraction::new(105, 127).normalize(&source_interval, MinIsMaxBehavior::PreferZero),
            Fraction::new(5, 20)
        );
        assert_eq!(
            Fraction::new(100, 127).normalize(&source_interval, MinIsMaxBehavior::PreferZero),
            Fraction::new(0, 20)
        );
        assert_eq!(
            Fraction::new(50, 127).normalize(&source_interval, MinIsMaxBehavior::PreferZero),
            Fraction::new(0, 20)
        );
        assert_eq!(
            Fraction::new(120, 127).normalize(&source_interval, MinIsMaxBehavior::PreferZero),
            Fraction::new(20, 20)
        );
        assert_eq!(
            Fraction::new(127, 127).normalize(&source_interval, MinIsMaxBehavior::PreferZero),
            Fraction::new(20, 20)
        );
    }

    #[test]
    fn normalize_intersection() {
        // Given
        let source_interval = Interval::new(10, 100);
        // When
        // Then
        assert_eq!(
            Fraction::new(0, 20).normalize(&source_interval, MinIsMaxBehavior::PreferZero),
            Fraction::new(0, 10)
        );
        assert_eq!(
            Fraction::new(10, 20).normalize(&source_interval, MinIsMaxBehavior::PreferZero),
            Fraction::new(0, 10)
        );
        assert_eq!(
            Fraction::new(15, 20).normalize(&source_interval, MinIsMaxBehavior::PreferZero),
            Fraction::new(5, 10)
        );
        assert_eq!(
            Fraction::new(20, 20).normalize(&source_interval, MinIsMaxBehavior::PreferZero),
            Fraction::new(10, 10)
        );
        assert_eq!(
            Fraction::new(127, 20).normalize(&source_interval, MinIsMaxBehavior::PreferZero),
            Fraction::new(10, 10)
        );
    }

    #[test]
    fn denormalize_subset() {
        // Given
        let source_interval = Interval::new(100, 120);
        // When
        // Then
        assert_eq!(
            Fraction::new(5, 127).denormalize(&source_interval, Some(130)),
            Fraction::new(105, 130)
        );
        assert_eq!(
            Fraction::new(0, 127).denormalize(&source_interval, Some(130)),
            Fraction::new(100, 130)
        );
        assert_eq!(
            Fraction::new(20, 127).denormalize(&source_interval, Some(130)),
            Fraction::new(120, 130)
        );
        assert_eq!(
            Fraction::new(30, 127).denormalize(&source_interval, Some(130)),
            Fraction::new(120, 130)
        );
        assert_eq!(
            Fraction::new(5, 127).denormalize(&source_interval, Some(110)),
            Fraction::new(105, 110)
        );
        assert_eq!(
            Fraction::new(0, 127).denormalize(&source_interval, Some(110)),
            Fraction::new(100, 110)
        );
        assert_eq!(
            Fraction::new(20, 127).denormalize(&source_interval, Some(110)),
            Fraction::new(110, 110)
        );
        assert_eq!(
            Fraction::new(30, 127).denormalize(&source_interval, Some(110)),
            Fraction::new(110, 110)
        );
        assert_eq!(
            Fraction::new(30, 127).denormalize(&source_interval, None),
            Fraction::new(120, 127)
        );
    }

    #[test]
    fn denormalize_intersection() {
        // Given
        let source_interval = Interval::new(10, 100);
        // When
        // Then
        assert_eq!(
            Fraction::new(0, 20).denormalize(&source_interval, Some(40)),
            Fraction::new(10, 40)
        );
        assert_eq!(
            Fraction::new(5, 20).denormalize(&source_interval, Some(40)),
            Fraction::new(15, 40)
        );
        assert_eq!(
            Fraction::new(10, 20).denormalize(&source_interval, Some(40)),
            Fraction::new(20, 40)
        );
        assert_eq!(
            Fraction::new(15, 20).denormalize(&source_interval, Some(40)),
            Fraction::new(25, 40)
        );
        assert_eq!(
            Fraction::new(0, 20).denormalize(&source_interval, Some(17)),
            Fraction::new(10, 17)
        );
        assert_eq!(
            Fraction::new(5, 20).denormalize(&source_interval, Some(17)),
            Fraction::new(15, 17)
        );
        assert_eq!(
            Fraction::new(10, 20).denormalize(&source_interval, Some(17)),
            Fraction::new(17, 17)
        );
        assert_eq!(
            Fraction::new(15, 20).denormalize(&source_interval, Some(17)),
            Fraction::new(17, 17)
        );
        assert_eq!(
            Fraction::new(15, 20).denormalize(&source_interval, None),
            Fraction::new(20, 20)
        );
    }
}
