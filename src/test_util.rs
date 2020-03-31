use crate::{ControlValue, UnitValue};
use approx::AbsDiffEq;

impl AbsDiffEq for UnitValue {
    type Epsilon = f64;

    fn default_epsilon() -> f64 {
        std::f64::EPSILON
    }

    fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        self.get_number().abs_diff_eq(&other.get_number(), epsilon)
    }
}

impl AbsDiffEq for ControlValue {
    type Epsilon = f64;

    fn default_epsilon() -> f64 {
        std::f64::EPSILON
    }

    fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        match (self, other) {
            (ControlValue::Absolute(v1), ControlValue::Absolute(v2)) => v1.abs_diff_eq(v2, epsilon),
            _ => self == other,
        }
    }
}
