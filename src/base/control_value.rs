use crate::{
    ControlType, DiscreteIncrement, Fraction, Interval, IntervalMatchResult, MinIsMaxBehavior,
    Transformation, UnitValue,
};
use std::cmp::Ordering;

/// Value coming from a source (e.g. a MIDI source) which is supposed to control something.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ControlValue {
    /// Absolute value that represents a percentage (e.g. fader position on the scale from lowest to
    /// highest, knob position on the scale from closed to fully opened, key press on the scale from
    /// not pressed to pressed with full velocity, key release).
    AbsoluteContinuous(UnitValue),
    /// Absolute value that is capable of retaining the original discrete value, e.g. the played
    /// note number, without immediately converting it into a UnitValue and thereby losing that
    /// information - which is important for the new "Discrete" mode.
    AbsoluteDiscrete(Fraction),
    /// Relative increment (e.g. encoder movement)
    Relative(DiscreteIncrement),
}

impl ControlValue {
    /// Convenience method for creating an absolute control value
    pub fn absolute_continuous(number: f64) -> ControlValue {
        ControlValue::AbsoluteContinuous(UnitValue::new(number))
    }

    /// Convenience method for creating a discrete absolute control value
    pub fn absolute_discrete(actual: u32, max: u32) -> ControlValue {
        ControlValue::AbsoluteDiscrete(Fraction::new(actual, max))
    }

    /// Convenience method for creating a relative control value
    pub fn relative(increment: i32) -> ControlValue {
        ControlValue::Relative(DiscreteIncrement::new(increment))
    }

    pub fn from_absolute(value: AbsoluteValue) -> ControlValue {
        match value {
            AbsoluteValue::Continuous(v) => Self::AbsoluteContinuous(v),
            AbsoluteValue::Discrete(f) => Self::AbsoluteDiscrete(f),
        }
    }

    /// Extracts the unit value if this is an absolute control value.
    pub fn as_unit_value(self) -> Result<UnitValue, &'static str> {
        match self {
            ControlValue::AbsoluteContinuous(v) => Ok(v),
            ControlValue::AbsoluteDiscrete(f) => Ok(f.to_unit_value()),
            _ => Err("control value is not absolute"),
        }
    }

    /// Extracts the discrete increment if this is a relative control value.
    pub fn as_discrete_increment(self) -> Result<DiscreteIncrement, &'static str> {
        match self {
            ControlValue::Relative(v) => Ok(v),
            _ => Err("control value is not relative"),
        }
    }

    pub fn inverse(self) -> ControlValue {
        match self {
            ControlValue::AbsoluteContinuous(v) => ControlValue::AbsoluteContinuous(v.inverse()),
            ControlValue::Relative(v) => ControlValue::Relative(v.inverse()),
            ControlValue::AbsoluteDiscrete(v) => ControlValue::AbsoluteDiscrete(v.inverse()),
        }
    }

    pub fn to_absolute_continuous(self) -> Result<ControlValue, &'static str> {
        match self {
            ControlValue::AbsoluteContinuous(v) => Ok(ControlValue::AbsoluteContinuous(v)),
            ControlValue::Relative(_) => Err("relative value can't be normalized"),
            ControlValue::AbsoluteDiscrete(v) => {
                Ok(ControlValue::AbsoluteContinuous(v.to_unit_value()))
            }
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum AbsoluteValue {
    Continuous(UnitValue),
    Discrete(Fraction),
}

impl PartialOrd for AbsoluteValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.to_unit_value().partial_cmp(&other.to_unit_value())
    }
}

impl PartialEq for AbsoluteValue {
    fn eq(&self, other: &Self) -> bool {
        self.to_unit_value() == other.to_unit_value()
    }
}

impl Ord for AbsoluteValue {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_unit_value().cmp(&other.to_unit_value())
    }
}

impl Eq for AbsoluteValue {}

impl AbsoluteValue {
    pub fn is_on(&self) -> bool {
        !self.is_zero()
    }

    pub fn to_unit_value(self) -> UnitValue {
        match self {
            AbsoluteValue::Continuous(v) => v,
            AbsoluteValue::Discrete(v) => v.to_unit_value(),
        }
    }

    pub fn is_zero(&self) -> bool {
        match self {
            AbsoluteValue::Continuous(v) => v.is_zero(),
            AbsoluteValue::Discrete(v) => v.is_zero(),
        }
    }

    pub fn is_continuous(&self) -> bool {
        matches!(self, AbsoluteValue::Continuous(_))
    }

    pub fn matches_tolerant(
        self,
        continuous_interval: &Interval<UnitValue>,
        discrete_interval: &Interval<u32>,
        epsilon: f64,
    ) -> IntervalMatchResult {
        match self {
            AbsoluteValue::Continuous(v) => continuous_interval.value_matches_tolerant(v, epsilon),
            AbsoluteValue::Discrete(v) => discrete_interval.value_matches(v.actual()),
        }
    }

    pub fn select_appropriate_interval_min(
        self,
        continuous_interval: &Interval<UnitValue>,
        discrete_interval: &Interval<u32>,
    ) -> AbsoluteValue {
        use AbsoluteValue::*;
        match self {
            Continuous(_) => Continuous(continuous_interval.min_val()),
            Discrete(v) => Discrete(v.with_actual(discrete_interval.min_val())),
        }
    }

    pub fn select_appropriate_interval_max(
        self,
        continuous_interval: &Interval<UnitValue>,
        discrete_interval: &Interval<u32>,
    ) -> AbsoluteValue {
        use AbsoluteValue::*;
        match self {
            Continuous(_) => Continuous(continuous_interval.max_val()),
            Discrete(v) => Discrete(v.with_actual(discrete_interval.max_val())),
        }
    }

    /// Normalizes this value with regard to the given interval.
    ///
    /// This value should be in the given interval!
    ///
    /// - Continuous: Scales to unit interval (= scales up = decreases resolution).
    /// - Discrete: Uses the interval minimum as zero.
    pub fn normalize(
        self,
        continuous_interval: &Interval<UnitValue>,
        discrete_interval: &Interval<u32>,
        min_is_max_behavior: MinIsMaxBehavior,
        is_discrete_mode: bool,
        epsilon: f64,
    ) -> Self {
        use AbsoluteValue::*;
        match self {
            Continuous(v) => {
                let scaled = v.normalize(continuous_interval, min_is_max_behavior, epsilon);
                Continuous(scaled)
            }
            Discrete(v) => {
                if is_discrete_mode {
                    // Normalize without scaling.
                    let rooted = v.normalize(discrete_interval, min_is_max_behavior);
                    Discrete(rooted)
                } else if continuous_interval.is_full() {
                    // Retain discreteness of value even in non-discrete mode if this is a no-op!
                    Discrete(v)
                } else {
                    // Use scaling if we are in non-discrete mode, thereby destroying the
                    // value's discreteness.
                    let scaled = v.to_unit_value().normalize(
                        continuous_interval,
                        min_is_max_behavior,
                        epsilon,
                    );
                    Continuous(scaled)
                }
            }
        }
    }

    /// Denormalizes this value with regard to the given interval.
    ///
    /// This value should be normalized!
    ///
    /// - Continuous: Scales from unit interval (= scales down = increases resolution).
    /// - Discrete: Adds the interval minimum.
    pub fn denormalize(
        self,
        continuous_interval: &Interval<UnitValue>,
        discrete_interval: &Interval<u32>,
        is_discrete_mode: bool,
    ) -> Self {
        use AbsoluteValue::*;
        match self {
            Continuous(v) => {
                let scaled = v.denormalize(continuous_interval);
                Continuous(scaled)
            }
            Discrete(v) => {
                if is_discrete_mode {
                    // Denormalize without scaling.
                    let unrooted = v.denormalize(discrete_interval);
                    Discrete(unrooted)
                } else if continuous_interval.is_full() {
                    // Retain discreteness of value even in non-discrete mode if this is a no-op!
                    Discrete(v)
                } else {
                    // Use scaling if we are in non-discrete mode, thereby destroying the
                    // value's discreteness.
                    let scaled = v.to_unit_value().denormalize(continuous_interval);
                    Continuous(scaled)
                }
            }
        }
    }

    pub fn transform<T: Transformation>(
        self,
        transformation: &T,
        current_target_value: Option<AbsoluteValue>,
        is_discrete_mode: bool,
    ) -> Result<Self, &'static str> {
        use AbsoluteValue::*;
        match self {
            Continuous(v) => {
                // Input value is continuous.
                let current_target_value = current_target_value
                    .map(|t| t.to_unit_value())
                    .unwrap_or_default();
                let res = transformation.transform_continuous(v, current_target_value)?;
                Ok(Continuous(res))
            }
            Discrete(v) => {
                // Input value is discrete.
                let current_target_value = current_target_value
                    .unwrap_or_else(|| AbsoluteValue::Discrete(v.with_actual(0)));
                match current_target_value {
                    Continuous(t) => {
                        // Target value is continuous.
                        let res = transformation.transform_continuous(v.to_unit_value(), t)?;
                        Ok(Continuous(res))
                    }
                    Discrete(t) => {
                        // Target value is also discrete.
                        if is_discrete_mode {
                            // Discrete mode.
                            // Transform using non-normalized rounded floating point values.
                            let res = transformation.transform_discrete(v, t)?;
                            Ok(Discrete(res))
                        } else {
                            // Continuous mode.
                            // Transform using normalized floating point values, thereby destroying
                            // the value's discreteness.
                            let res = transformation
                                .transform_continuous(v.to_unit_value(), t.to_unit_value())?;
                            Ok(Continuous(res))
                        }
                    }
                }
            }
        }
    }

    pub fn inverse(self, discrete_max: u32) -> Self {
        use AbsoluteValue::*;
        match self {
            Continuous(v) => Self::Continuous(v.inverse()),
            Discrete(f) => Self::Discrete(f.with_max(discrete_max).inverse()),
        }
    }

    pub fn round(self, control_type: ControlType) -> Self {
        use AbsoluteValue::*;
        match self {
            Continuous(v) => {
                let value = round_to_nearest_discrete_value(control_type, v);
                Self::Continuous(value)
            }
            Discrete(f) => Self::Discrete(f),
        }
    }

    pub fn has_same_effect_as(self, other: AbsoluteValue) -> bool {
        if let (AbsoluteValue::Discrete(f1), AbsoluteValue::Discrete(f2)) = (self, other) {
            f1.actual() == f2.actual()
        } else {
            self.to_unit_value() == other.to_unit_value()
        }
    }

    pub fn calc_distance_from(self, rhs: Self) -> Self {
        use AbsoluteValue::*;
        match (self, rhs) {
            (Discrete(f1), Discrete(f2)) => {
                let distance = (f2.actual() as i32 - f1.actual() as i32).abs() as u32;
                Self::Discrete(Fraction::new_max(distance))
            }
            _ => {
                let distance = self.to_unit_value().calc_distance_from(rhs.to_unit_value());
                Self::Continuous(distance)
            }
        }
    }

    pub fn is_greater_than(&self, continuous_jump_max: UnitValue, discrete_jump_max: u32) -> bool {
        use AbsoluteValue::*;
        match self {
            Continuous(d) => *d > continuous_jump_max,
            Discrete(d) => d.actual() > discrete_jump_max,
        }
    }

    pub fn is_lower_than(&self, continuous_jump_min: UnitValue, discrete_jump_min: u32) -> bool {
        use AbsoluteValue::*;
        match self {
            Continuous(d) => *d < continuous_jump_min,
            Discrete(d) => d.actual() < discrete_jump_min,
        }
    }
}

impl Default for AbsoluteValue {
    fn default() -> Self {
        Self::Continuous(Default::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BASE_EPSILON;
    use approx::*;

    #[test]
    fn normalize_comparison() {
        // Given
        let continuous = AbsoluteValue::Continuous(UnitValue::new(105.0 / 127.0));
        let continuous_interval =
            Interval::new(UnitValue::new(100.0 / 127.0), UnitValue::new(120.0 / 127.0));
        let discrete = AbsoluteValue::Discrete(Fraction::new(105, 127));
        let discrete_interval = Interval::new(100, 120);
        // When
        let continuous_normalized = continuous.normalize(
            &continuous_interval,
            &discrete_interval,
            MinIsMaxBehavior::PreferZero,
            true,
            BASE_EPSILON,
        );
        let discrete_normalized = discrete.normalize(
            &continuous_interval,
            &discrete_interval,
            MinIsMaxBehavior::PreferZero,
            true,
            BASE_EPSILON,
        );
        // Then
        assert_abs_diff_eq!(
            continuous_normalized.to_unit_value().get(),
            0.25,
            epsilon = BASE_EPSILON
        );
        assert_eq!(
            discrete_normalized,
            AbsoluteValue::Discrete(Fraction::new(5, 20))
        );
        assert_abs_diff_eq!(
            discrete_normalized.to_unit_value().get(),
            0.25,
            epsilon = BASE_EPSILON
        );
    }

    #[test]
    fn denormalize_comparison() {
        // Given
        let continuous = AbsoluteValue::Continuous(UnitValue::new(105.0 / 127.0));
        let continuous_interval = Interval::new(
            UnitValue::new(100.0 / 1000.0),
            UnitValue::new(500.0 / 1000.0),
        );
        let discrete = AbsoluteValue::Discrete(Fraction::new(105, 127));
        let discrete_interval = Interval::new(100, 500);
        // When
        let continuous_normalized =
            continuous.denormalize(&continuous_interval, &discrete_interval, true);
        let discrete_normalized =
            discrete.denormalize(&continuous_interval, &discrete_interval, true);
        // Then
        assert_abs_diff_eq!(
            continuous_normalized.to_unit_value().get(),
            0.4307086614173229,
            epsilon = BASE_EPSILON
        );
        assert_eq!(
            discrete_normalized,
            AbsoluteValue::Discrete(Fraction::new(205, 227))
        );
    }
}

fn round_to_nearest_discrete_value(
    control_type: ControlType,
    approximate_control_value: UnitValue,
) -> UnitValue {
    // round() is the right choice here vs. floor() because we don't want slight numerical
    // inaccuracies lead to surprising jumps
    use ControlType::*;
    let step_size = match control_type {
        AbsoluteContinuousRoundable { rounding_step_size } => rounding_step_size,
        AbsoluteDiscrete { atomic_step_size } => atomic_step_size,
        AbsoluteContinuousRetriggerable
        | AbsoluteContinuous
        | Relative
        | VirtualMulti
        | VirtualButton => {
            return approximate_control_value;
        }
    };
    approximate_control_value.snap_to_grid_by_interval_size(step_size)
}
