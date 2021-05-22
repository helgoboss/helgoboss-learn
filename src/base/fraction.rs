use crate::{Interval, IntervalMatchResult, MinIsMaxBehavior, UnitValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fraction {
    /// Concrete discrete value.
    actual: u32,
    /// Soft maximum value: Good to know in order to be able to instantly convert to a UnitValue
    /// whenever we want to go absolute-continuous.
    max: u32,
    // TODO-high Remove!!!
    pub unit_value: UnitValue,
}

impl Fraction {
    // TODO-high Make const
    pub fn new(actual: u32, max: u32) -> Self {
        Self {
            actual,
            max,
            unit_value: to_unit_value(actual, max),
        }
    }

    pub const fn new_min(max: u32) -> Self {
        Self {
            actual: 0,
            max,
            unit_value: UnitValue::MIN,
        }
    }

    pub const fn new_max(max: u32) -> Self {
        Self {
            actual: max,
            max,
            unit_value: UnitValue::MAX,
        }
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

    pub fn inverse(&self) -> Self {
        Self {
            actual: self.max - self.actual_clamped(),
            max: self.max,
            unit_value: self.unit_value.inverse(),
        }
    }

    pub fn is_zero(&self) -> bool {
        self.actual == 0
    }

    pub fn to_unit_value(&self) -> UnitValue {
        to_unit_value(self.actual, self.max)
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
        use IntervalMatchResult::*;
        let new_max = self.interval().intersect(interval).span();
        match interval.value_matches(self.actual) {
            Between => {
                let rooted_actual = self.actual - interval.min_val();
                Fraction::new(rooted_actual, new_max)
            }
            MinAndMax => {
                use MinIsMaxBehavior::*;
                match min_is_max_behavior {
                    PreferZero => Self::new_min(0),
                    PreferOne => Self::new_max(1),
                }
            }
            Min | Lower => Fraction::new_min(new_max),
            Max | Greater => Fraction::new_max(new_max),
        }
    }
}

// TODO-high Delete
impl From<Fraction> for UnitValue {
    fn from(f: Fraction) -> Self {
        f.to_unit_value()
    }
}

fn to_unit_value(actual: u32, max: u32) -> UnitValue {
    if max == 0 {
        return UnitValue::MIN;
    }
    UnitValue::new(std::cmp::min(actual, max) as f64 / max as f64)
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
}
