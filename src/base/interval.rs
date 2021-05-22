use std::fmt::Debug;
use std::ops::Sub;

/// An interval which has an inclusive min and inclusive max value.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Interval<T: PartialOrd + Copy + Sub + Debug> {
    min: T,
    max: T,
}

impl<T: PartialOrd + Copy + Sub + Debug> Interval<T> {
    /// Creates an interval. Panics if `min` is greater than `max`.
    pub fn new(min: T, max: T) -> Interval<T> {
        assert!(
            min <= max,
            "min = {:?} is greater than max = {:?}",
            min,
            max
        );
        Interval { min, max }
    }

    pub fn new_auto(bound_1: T, bound_2: T) -> Interval<T> {
        Interval {
            min: if bound_1 <= bound_2 { bound_1 } else { bound_2 },
            max: if bound_1 >= bound_2 { bound_1 } else { bound_2 },
        }
    }

    /// Checks if this interval contains the given value.
    ///
    /// **Attention:** This is very strict at the interval bounds and doesn't consider numerical
    /// inaccuracies. Consider using `value_matches_tolerant()` instead.
    pub fn contains(&self, value: T) -> bool {
        self.min <= value && value <= self.max
    }

    pub fn min_is_max(&self, epsilon: f64) -> bool
    where
        T: Sub<Output = f64>,
    {
        (self.max - self.min).abs() < epsilon
    }

    pub fn value_matches_tolerant(&self, value: T, epsilon: f64) -> IntervalMatchResult
    where
        T: Sub<Output = f64>,
    {
        let is_min = (self.min - value).abs() < epsilon;
        let is_max = (value - self.max).abs() < epsilon;
        self.value_matches_internal(value, is_min, is_max)
    }

    pub fn value_matches(&self, value: T) -> IntervalMatchResult {
        let is_min = value == self.min;
        let is_max = value == self.max;
        self.value_matches_internal(value, is_min, is_max)
    }

    fn value_matches_internal(&self, value: T, is_min: bool, is_max: bool) -> IntervalMatchResult {
        if is_min && is_max {
            IntervalMatchResult::MinAndMax
        } else if is_min {
            IntervalMatchResult::Min
        } else if is_max {
            IntervalMatchResult::Max
        } else if value < self.min {
            IntervalMatchResult::Lower
        } else if value > self.max {
            IntervalMatchResult::Greater
        } else {
            IntervalMatchResult::Between
        }
    }

    /// Returns the low bound of this interval.
    pub fn min_val(&self) -> T {
        self.min
    }

    /// Returns a new interval containing the given minimum.
    ///
    /// If the given minimum is greater than the current maximum, the maximum will be set to given
    /// minimum.
    pub fn with_min(&self, min: T) -> Interval<T> {
        Interval::new(min, if min <= self.max { self.max } else { min })
    }
    /// Returns a new interval containing the given maximum.
    ///
    /// If the given maximum is lower than the current minimum, the minimum will be set to the given
    /// maximum.
    pub fn with_max(&self, max: T) -> Interval<T> {
        Interval::new(if self.min <= max { self.min } else { max }, max)
    }

    /// Returns the high bound of this interval.
    pub fn max_val(&self) -> T {
        self.max
    }

    /// Returns the distance between the low and high bound of this interval.
    pub fn span(&self) -> T::Output {
        self.max - self.min
    }

    /// If there's no intersection, a zero interval (with default values) will be returned.
    pub fn intersect(&self, other: &Interval<T>) -> Interval<T>
    where
        T: Default,
    {
        let greatest_min = partial_min_max::max(self.min, other.min);
        let lowest_max = partial_min_max::min(self.max, other.max);
        if greatest_min <= lowest_max {
            Interval::new(greatest_min, lowest_max)
        } else {
            Interval::new(Default::default(), Default::default())
        }
    }
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum IntervalMatchResult {
    Between,
    Min,
    Max,
    MinAndMax,
    Lower,
    Greater,
}

impl IntervalMatchResult {
    pub fn matches(self) -> bool {
        use IntervalMatchResult::*;
        match self {
            Between | Min | Max | MinAndMax => true,
            Lower | Greater => false,
        }
    }
}
