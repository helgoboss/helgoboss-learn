use crate::{AbsoluteValue, ControlValue, UnitValue};
use approx::AbsDiffEq;

impl AbsDiffEq for UnitValue {
    type Epsilon = f64;

    fn default_epsilon() -> f64 {
        f64::EPSILON
    }

    fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        self.get().abs_diff_eq(&other.get(), epsilon)
    }
}

impl AbsDiffEq for ControlValue {
    type Epsilon = f64;

    fn default_epsilon() -> f64 {
        f64::EPSILON
    }

    fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        match (self, other) {
            (ControlValue::AbsoluteContinuous(v1), ControlValue::AbsoluteContinuous(v2)) => {
                v1.abs_diff_eq(v2, epsilon)
            }
            _ => self == other,
        }
    }
}

impl AbsDiffEq for AbsoluteValue {
    type Epsilon = f64;

    fn default_epsilon() -> f64 {
        f64::EPSILON
    }

    fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        match (self, other) {
            (AbsoluteValue::Continuous(v1), AbsoluteValue::Continuous(v2)) => {
                v1.abs_diff_eq(v2, epsilon)
            }
            _ => self == other,
        }
    }
}
