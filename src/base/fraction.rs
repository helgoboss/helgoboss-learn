use crate::UnitValue;

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
