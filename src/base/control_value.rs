use crate::{
    DiscreteIncrement, Fraction, Interval, IntervalMatchResult, MinIsMaxBehavior, Transformation,
    UnitValue, BASE_EPSILON,
};
use std::ops::Deref;

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
            ControlValue::AbsoluteDiscrete(f) => Ok(f.into()),
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
            ControlValue::AbsoluteDiscrete(v) => Ok(ControlValue::AbsoluteContinuous(v.into())),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum AbsoluteValue {
    Continuous(UnitValue),
    Discrete(Fraction),
}

impl AbsoluteValue {
    // TODO-high Maybe remove
    pub fn to_unit_value(self) -> UnitValue {
        match self {
            AbsoluteValue::Continuous(v) => v,
            AbsoluteValue::Discrete(v) => v.to_unit_value(),
        }
    }

    pub fn matches_tolerant(
        self,
        continuous_interval: &Interval<UnitValue>,
        discrete_interval: &Interval<u32>,
    ) -> IntervalMatchResult {
        match self {
            AbsoluteValue::Continuous(v) => {
                continuous_interval.value_matches_tolerant(v, BASE_EPSILON)
            }
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
    pub fn apply_source_interval(
        self,
        continuous_interval: &Interval<UnitValue>,
        discrete_interval: &Interval<u32>,
        min_is_max_behavior: MinIsMaxBehavior,
        is_discrete_mode: bool,
    ) -> Self {
        use AbsoluteValue::*;
        match self {
            Continuous(v) => {
                let scaled = v.normalize(continuous_interval, min_is_max_behavior, BASE_EPSILON);
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
                        BASE_EPSILON,
                    );
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
}

// TODO-high Remove!!!
impl Deref for AbsoluteValue {
    type Target = UnitValue;

    fn deref(&self) -> &Self::Target {
        match self {
            AbsoluteValue::Continuous(v) => &v,
            AbsoluteValue::Discrete(f) => &f.unit_value,
        }
    }
}

impl Default for AbsoluteValue {
    fn default() -> Self {
        Self::Continuous(Default::default())
    }
}
