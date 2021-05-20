use crate::UnitValue;

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
        Self { actual: 0, max }
    }

    pub const fn new_max(max: u32) -> Self {
        Self { actual: max, max }
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

    pub fn inverse(&self) -> Self {
        Self {
            actual: self.max - self.actual_clamped(),
            max: self.max,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.actual == 0
    }
}

impl From<Fraction> for UnitValue {
    fn from(f: Fraction) -> Self {
        if f.max == 0 {
            return Self::MIN;
        }
        UnitValue::new(f.actual_clamped() as f64 / f.max as f64)
    }
}
