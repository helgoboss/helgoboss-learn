use crate::UnitValue;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fraction {
    /// Concrete discrete value.
    actual: u32,
    /// Maximum value: Good to know in order to be able to instantly convert to a UnitValue whenever we
    /// want to go absolute-continuous.
    max: u32,
}

impl Fraction {
    pub fn new(actual: u32, max: u32) -> Result<Fraction, &'static str> {
        if actual > max {
            return Err("actual must not be greater than max");
        }
        let fraction = Fraction { actual, max };
        Ok(fraction)
    }

    pub fn actual(&self) -> u32 {
        self.actual
    }

    pub fn max(&self) -> u32 {
        self.max
    }

    pub fn inverse(&self) -> Self {
        Self {
            actual: self.max - self.actual,
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
        UnitValue::new(f.actual as f64 / f.max as f64)
    }
}
