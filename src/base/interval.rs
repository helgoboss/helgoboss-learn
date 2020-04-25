use std::ops::Sub;

/// An interval which has an inclusive min and inclusive max value.
#[derive(Clone, Debug)]
pub struct Interval<T: PartialOrd + Copy + Sub> {
    min: T,
    max: T,
}

impl<T: PartialOrd + Copy + Sub> Interval<T> {
    /// Creates an interval. Panics if `min` is greater than `max`.
    pub fn new(min: T, max: T) -> Interval<T> {
        assert!(min <= max);
        Interval { min, max }
    }

    /// Checks if this interval contains the given value.
    pub fn contains(&self, value: T) -> bool {
        self.min <= value && value <= self.max
    }

    /// Returns the low bound of this interval.
    pub fn min(&self) -> T {
        self.min
    }

    /// Returns the high bound of this interval.
    pub fn max(&self) -> T {
        self.max
    }

    /// Returns the distance between the low and high bound of this interval.
    pub fn span(&self) -> T::Output {
        self.max - self.min
    }
}
