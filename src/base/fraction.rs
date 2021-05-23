use crate::{Interval, IntervalMatchResult, MinIsMaxBehavior, UnitValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fraction {
    /// Concrete discrete value.
    actual: u32,
    /// Soft maximum value: Good to know in order to be able to instantly convert to a UnitValue
    /// whenever we want to go absolute-continuous.
    max: u32,
}

impl Fraction {
    pub const fn new(actual: u32, max: u32) -> Self {
        Self { actual, max }
    }

    pub const fn new_min(max: u32) -> Self {
        Self::new(0, max)
    }

    pub const fn new_max(max: u32) -> Self {
        Self::new(max, max)
    }

    pub fn actual(&self) -> u32 {
        self.actual
    }

    pub fn actual_clamped(&self) -> u32 {
        std::cmp::min(self.actual, self.max)
    }

    pub fn max(&self) -> u32 {
        self.max
    }

    pub fn with_actual(&self, actual: u32) -> Self {
        Self::new(actual, self.max)
    }

    pub fn with_max(&self, max: u32) -> Self {
        Self::new(self.actual, max)
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

    pub fn to_unit_value(&self) -> UnitValue {
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

    fn interval(&self) -> Interval<u32> {
        Interval::new(0, self.max)
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
    pub fn denormalize(&self, interval: &Interval<u32>) -> Self {
        let rooted_actual = self.actual;
        let rooted_max = self.max;
        // actual
        let unrooted_actual = interval.min_val() + rooted_actual;
        let unrooted_actual = std::cmp::min(unrooted_actual, interval.max_val());
        // max
        let min_span = std::cmp::min(rooted_max, interval.span());
        let unrooted_max = interval.min_val() + min_span;
        // fraction
        Fraction::new(unrooted_actual, unrooted_max)
    }
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
            Fraction::new(5, 127).denormalize(&source_interval),
            Fraction::new(105, 120)
        );
        assert_eq!(
            Fraction::new(0, 127).denormalize(&source_interval),
            Fraction::new(100, 120)
        );
        assert_eq!(
            Fraction::new(20, 127).denormalize(&source_interval),
            Fraction::new(120, 120)
        );
        assert_eq!(
            Fraction::new(30, 127).denormalize(&source_interval),
            Fraction::new(120, 120)
        );
    }

    #[test]
    fn denormalize_intersection() {
        // Given
        let source_interval = Interval::new(10, 100);
        // When
        // Then
        assert_eq!(
            Fraction::new(0, 20).denormalize(&source_interval),
            Fraction::new(10, 30)
        );
        assert_eq!(
            Fraction::new(5, 20).denormalize(&source_interval),
            Fraction::new(15, 30)
        );
        assert_eq!(
            Fraction::new(10, 20).denormalize(&source_interval),
            Fraction::new(20, 30)
        );
        assert_eq!(
            Fraction::new(15, 20).denormalize(&source_interval),
            Fraction::new(25, 30)
        );
    }
}
