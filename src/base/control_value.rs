use crate::{
    ControlType, DiscreteIncrement, Fraction, Interval, IntervalMatchResult, MinIsMaxBehavior,
    Transformation, TransformationInput, TransformationInputContext, TransformationInputEvent,
    TransformationInstruction, UnitIncrement, UnitValue, BASE_EPSILON,
};
use num_enum::TryFromPrimitive;
// Use once_cell::sync::Lazy instead of std::sync::LazyLock to be able to build with Rust 1.77.2 (to stay Win7-compatible)
use once_cell::sync::Lazy as LazyLock;
use std::fmt::{Display, Formatter};
use std::ops::Sub;
use std::time::{Duration, Instant};

/// The timestamp is intended to be used for things like takeover modes. Ideally, the event
/// time should be captured when the event occurs but it's also okay to do that somewhat later
/// in the same callstack because event processing within the same thread happens very fast.
/// Most importantly, if the event is sent to another thread, then the time should be captured
/// *before* the event leaves the thread and saved. That allows more accurate processing in the
/// destination thread.  
pub trait AbstractTimestamp: Copy + Sub<Output = Duration> + std::fmt::Debug {
    fn duration(&self) -> Duration;
}

/// A timestamp that does nothing and takes no space.
#[derive(Copy, Clone, Debug, Default)]
pub struct NoopTimestamp;

impl AbstractTimestamp for NoopTimestamp {
    fn duration(&self) -> Duration {
        Duration::ZERO
    }
}

impl Sub for NoopTimestamp {
    type Output = Duration;

    fn sub(self, _: Self) -> Duration {
        Duration::ZERO
    }
}

impl AbstractTimestamp for Instant {
    fn duration(&self) -> Duration {
        static INSTANT: LazyLock<Instant> = LazyLock::new(Instant::now);
        self.saturating_duration_since(*INSTANT)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ControlEvent<P, T: AbstractTimestamp> {
    payload: P,
    timestamp: T,
}

impl<P: Display, T: AbstractTimestamp + Display> Display for ControlEvent<P, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}", &self.payload, self.timestamp)
    }
}

impl<P, T: AbstractTimestamp> ControlEvent<P, T> {
    /// Creates an event.
    pub fn new(payload: P, timestamp: T) -> Self {
        Self { timestamp, payload }
    }

    /// Returns the timestamp of this event.
    pub fn timestamp(&self) -> T {
        self.timestamp
    }

    /// Returns the payload of this event.
    pub fn payload(&self) -> P
    where
        P: Copy,
    {
        self.payload
    }

    /// Consumes this event and returns the payload.
    pub fn into_payload(self) -> P {
        self.payload
    }

    /// Replaces the payload of this event but keeps the timestamp.
    pub fn with_payload<O>(&self, payload: O) -> ControlEvent<O, T> {
        ControlEvent {
            timestamp: self.timestamp,
            payload,
        }
    }

    /// Transforms the payload of this event.
    pub fn map_payload<O>(self, map: impl FnOnce(P) -> O) -> ControlEvent<O, T> {
        let transformed_payload = map(self.payload);
        ControlEvent {
            timestamp: self.timestamp,
            payload: transformed_payload,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default, TryFromPrimitive)]
#[repr(u8)]
pub enum ControlValueKind {
    #[default]
    AbsoluteContinuous = 0,
    RelativeDiscrete = 1,
    RelativeContinuous = 2,
    AbsoluteDiscrete = 3,
}

/// Value coming from a source (e.g. a MIDI source) which is supposed to control something.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ControlValue {
    /// Absolute value that represents a percentage (e.g. fader position on the scale from lowest to
    /// highest, knob position on the scale from closed to fully opened, key press on the scale from
    /// not pressed to pressed with full velocity, key release).
    AbsoluteContinuous(UnitValue),
    /// Relative increment that represents a number of increments/decrements.
    RelativeDiscrete(DiscreteIncrement),
    /// Relative increment that represents a continuous adjustment.
    RelativeContinuous(UnitIncrement),
    /// Absolute value that is capable of retaining the original discrete value, e.g. the played
    /// note number, without immediately converting it into a UnitValue and thereby losing that
    /// information - which is important for the new "Discrete" mode.
    AbsoluteDiscrete(Fraction),
}

impl Display for ControlValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ControlValue::AbsoluteContinuous(v) => v.fmt(f),
            ControlValue::AbsoluteDiscrete(v) => v.fmt(f),
            ControlValue::RelativeContinuous(v) => v.fmt(f),
            ControlValue::RelativeDiscrete(v) => v.fmt(f),
        }
    }
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
        ControlValue::RelativeDiscrete(DiscreteIncrement::new(increment))
    }

    pub fn from_absolute(value: AbsoluteValue) -> ControlValue {
        match value {
            AbsoluteValue::Continuous(v) => Self::AbsoluteContinuous(v),
            AbsoluteValue::Discrete(f) => Self::AbsoluteDiscrete(f),
        }
    }

    pub fn from_relative(increment: Increment) -> ControlValue {
        match increment {
            Increment::Continuous(i) => Self::RelativeContinuous(i),
            Increment::Discrete(i) => Self::RelativeDiscrete(i),
        }
    }

    /// Extracts the unit value if this is an absolute control value.
    pub fn to_unit_value(self) -> Result<UnitValue, &'static str> {
        match self {
            ControlValue::AbsoluteContinuous(v) => Ok(v),
            ControlValue::AbsoluteDiscrete(f) => Ok(f.to_unit_value()),
            _ => Err("control value is not absolute"),
        }
    }

    /// Extracts the discrete value if this is an absolute control value.
    ///
    /// The `value_count` is only used if this value is a unit value, in order to transform it into a discrete value.
    pub fn to_discrete_value(self, value_count: u32) -> Result<Fraction, &'static str> {
        match self {
            ControlValue::AbsoluteContinuous(v) => {
                if value_count == 0 {
                    return Ok(Fraction::new_max(0));
                }
                let actual = (v.get() * (value_count - 1) as f64).round() as u32;
                Ok(Fraction::new(actual, value_count))
            }
            ControlValue::AbsoluteDiscrete(f) => Ok(f),
            _ => Err("control value is not absolute"),
        }
    }

    /// Extracts an absolute value if this is an absolute control value.
    pub fn to_absolute_value(self) -> Result<AbsoluteValue, &'static str> {
        match self {
            ControlValue::AbsoluteContinuous(v) => Ok(AbsoluteValue::Continuous(v)),
            ControlValue::AbsoluteDiscrete(f) => Ok(AbsoluteValue::Discrete(f)),
            _ => Err("control value is not absolute"),
        }
    }

    /// Extracts the discrete increment if this is a relative control value.
    pub fn as_discrete_increment(self) -> Result<DiscreteIncrement, &'static str> {
        match self {
            ControlValue::RelativeDiscrete(v) => Ok(v),
            _ => Err("control value is not relative"),
        }
    }

    pub fn inverse(self) -> ControlValue {
        match self {
            ControlValue::AbsoluteContinuous(v) => ControlValue::AbsoluteContinuous(v.inverse()),
            ControlValue::RelativeDiscrete(v) => ControlValue::RelativeDiscrete(v.inverse()),
            ControlValue::RelativeContinuous(v) => ControlValue::RelativeContinuous(v.inverse()),
            ControlValue::AbsoluteDiscrete(v) => ControlValue::AbsoluteDiscrete(v.inverse()),
        }
    }

    pub fn to_absolute_continuous(self) -> Result<ControlValue, &'static str> {
        match self {
            ControlValue::AbsoluteContinuous(v) => Ok(ControlValue::AbsoluteContinuous(v)),
            ControlValue::AbsoluteDiscrete(v) => {
                Ok(ControlValue::AbsoluteContinuous(v.to_unit_value()))
            }
            ControlValue::RelativeContinuous(_) | ControlValue::RelativeDiscrete(_) => {
                Err("relative values can't be normalized")
            }
        }
    }

    pub fn is_on(self) -> bool {
        self.to_unit_value()
            .map(|uv| !uv.is_zero())
            .unwrap_or(false)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum AbsoluteValue {
    Continuous(UnitValue),
    Discrete(Fraction),
}

impl AbsoluteValue {
    pub fn from_bool(on: bool) -> Self {
        if on {
            AbsoluteValue::Continuous(UnitValue::MAX)
        } else {
            AbsoluteValue::Continuous(UnitValue::MIN)
        }
    }

    pub fn is_on(&self) -> bool {
        !self.is_zero()
    }

    pub fn continuous_value(self) -> Option<UnitValue> {
        match self {
            AbsoluteValue::Continuous(v) => Some(v),
            AbsoluteValue::Discrete(_) => None,
        }
    }

    pub fn discrete_value(self) -> Option<Fraction> {
        match self {
            AbsoluteValue::Continuous(_) => None,
            AbsoluteValue::Discrete(f) => Some(f),
        }
    }

    pub fn to_unit_value(self) -> UnitValue {
        match self {
            AbsoluteValue::Continuous(v) => v,
            AbsoluteValue::Discrete(v) => v.to_unit_value(),
        }
    }

    pub fn to_continuous_value(self) -> AbsoluteValue {
        AbsoluteValue::Continuous(self.to_unit_value())
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
        use_discrete_processing: bool,
        epsilon: f64,
    ) -> IntervalMatchResult {
        match self {
            AbsoluteValue::Continuous(v) => continuous_interval.value_matches_tolerant(v, epsilon),
            AbsoluteValue::Discrete(v) => {
                if use_discrete_processing {
                    discrete_interval.value_matches(v.actual())
                } else {
                    continuous_interval.value_matches_tolerant(v.to_unit_value(), epsilon)
                }
            }
        }
    }

    pub fn select_appropriate_interval_min(
        self,
        continuous_interval: &Interval<UnitValue>,
        discrete_interval: &Interval<u32>,
    ) -> AbsoluteValue {
        use AbsoluteValue as V;
        match self {
            V::Continuous(_) => V::Continuous(continuous_interval.min_val()),
            V::Discrete(v) => V::Discrete(v.with_actual(discrete_interval.min_val())),
        }
    }

    pub fn select_appropriate_interval_max(
        self,
        continuous_interval: &Interval<UnitValue>,
        discrete_interval: &Interval<u32>,
    ) -> AbsoluteValue {
        use AbsoluteValue as V;
        match self {
            V::Continuous(_) => V::Continuous(continuous_interval.max_val()),
            V::Discrete(v) => V::Discrete(v.with_actual(discrete_interval.max_val())),
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
        use AbsoluteValue as V;
        match self {
            V::Continuous(v) => {
                let scaled = v.normalize(continuous_interval, min_is_max_behavior, epsilon);
                V::Continuous(scaled)
            }
            V::Discrete(v) => {
                if is_discrete_mode {
                    // Normalize without scaling.
                    let rooted = v.normalize(discrete_interval, min_is_max_behavior);
                    V::Discrete(rooted)
                } else if continuous_interval.is_full() {
                    // Retain discreteness of value even in non-discrete mode if this is a no-op!
                    V::Discrete(v)
                } else {
                    // Use scaling if we are in non-discrete mode, thereby destroying the
                    // value's discreteness.
                    let scaled = v.to_unit_value().normalize(
                        continuous_interval,
                        min_is_max_behavior,
                        epsilon,
                    );
                    V::Continuous(scaled)
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
        discrete_max: Option<u32>,
    ) -> Self {
        use AbsoluteValue as V;
        match self {
            V::Continuous(v) => {
                let scaled = v.denormalize(continuous_interval);
                V::Continuous(scaled)
            }
            V::Discrete(v) => {
                if is_discrete_mode {
                    // Denormalize without scaling.
                    let unrooted = v.denormalize(discrete_interval, discrete_max);
                    V::Discrete(unrooted)
                } else if continuous_interval.is_full() {
                    // Retain discreteness of value even in non-discrete mode if this is a no-op!
                    V::Discrete(v)
                } else {
                    // Use scaling if we are in non-discrete mode, thereby destroying the
                    // value's discreteness.
                    let scaled = v.to_unit_value().denormalize(continuous_interval);
                    V::Continuous(scaled)
                }
            }
        }
    }

    pub fn transform<T: Transformation>(
        self,
        transformation: &T,
        current_target_value: Option<AbsoluteValue>,
        is_discrete_mode: bool,
        rel_time: Duration,
        timestamp: Duration,
        additional_input: T::AdditionalInput,
    ) -> Result<EnhancedTransformationOutput<ControlValue>, &'static str> {
        use AbsoluteValue as V;
        match self {
            V::Continuous(v) => {
                // Input value is continuous.
                let current_target_value = current_target_value
                    .map(|t| t.to_unit_value())
                    .unwrap_or_default();
                self.transform_continuous(
                    transformation,
                    v,
                    current_target_value,
                    rel_time,
                    timestamp,
                    additional_input,
                )
            }
            V::Discrete(v) => {
                // Input value is discrete.
                let current_target_value = current_target_value
                    .unwrap_or_else(|| AbsoluteValue::Discrete(v.with_actual(0)));
                match current_target_value {
                    V::Continuous(t) => {
                        // Target value is continuous.
                        self.transform_continuous(
                            transformation,
                            v.to_unit_value(),
                            t,
                            rel_time,
                            timestamp,
                            additional_input,
                        )
                    }
                    V::Discrete(t) => {
                        // Target value is also discrete.
                        if is_discrete_mode {
                            // Discrete mode.
                            // Transform using non-normalized rounded floating point values.
                            self.transform_discrete(
                                transformation,
                                v,
                                t,
                                rel_time,
                                timestamp,
                                additional_input,
                            )
                        } else {
                            // Continuous mode.
                            // Transform using normalized floating point values, thereby destroying
                            // the value's discreteness.
                            self.transform_continuous(
                                transformation,
                                v.to_unit_value(),
                                t.to_unit_value(),
                                rel_time,
                                timestamp,
                                additional_input,
                            )
                        }
                    }
                }
            }
        }
    }

    fn transform_continuous<T: Transformation>(
        self,
        transformation: &T,
        input_value: UnitValue,
        output_value: UnitValue,
        rel_time: Duration,
        timestamp: Duration,
        additional_input: T::AdditionalInput,
    ) -> Result<EnhancedTransformationOutput<ControlValue>, &'static str> {
        let input = TransformationInput {
            event: TransformationInputEvent {
                input_value: input_value.get(),
                timestamp,
            },
            context: TransformationInputContext {
                output_value: output_value.get(),
                rel_time,
            },
            additional_input,
        };
        let output = transformation.transform(input)?;
        let output = EnhancedTransformationOutput {
            produced_kind: output.produced_kind,
            value: output.extract_control_value(None),
            instruction: output.instruction,
        };
        Ok(output)
    }

    // Not currently used as discrete control not yet unlocked.
    fn transform_discrete<T: Transformation>(
        self,
        transformation: &T,
        input_value: Fraction,
        output_value: Fraction,
        rel_time: Duration,
        timestamp: Duration,
        additional_input: T::AdditionalInput,
    ) -> Result<EnhancedTransformationOutput<ControlValue>, &'static str> {
        let input = TransformationInput {
            event: TransformationInputEvent {
                input_value: input_value.actual() as _,
                timestamp,
            },
            context: TransformationInputContext {
                output_value: output_value.actual() as _,
                rel_time,
            },
            additional_input,
        };
        let output = transformation.transform(input)?;
        let out = EnhancedTransformationOutput {
            produced_kind: output.produced_kind,
            value: output.extract_control_value(Some(input_value.max_val())),
            instruction: output.instruction,
        };
        Ok(out)
    }

    pub fn inverse(self, new_discrete_max: Option<u32>) -> Self {
        use AbsoluteValue as V;
        match self {
            V::Continuous(v) => Self::Continuous(v.inverse()),
            // 100/100 (max 150) =>   0/150
            //   0/100 (max 150) => 100/150
            // 100/100 (max 50) =>    0/50
            //   0/100 (max 50) =>   50/50
            V::Discrete(f) => {
                let res = if let Some(new_max) = new_discrete_max {
                    let min_max = std::cmp::min(new_max, f.max_val());
                    let inversed_with_min_max = f.with_max_clamped(min_max).inverse();
                    inversed_with_min_max.with_max(new_max)
                } else {
                    f.inverse()
                };
                Self::Discrete(res)
            }
        }
    }

    pub fn round(self, control_type: ControlType) -> Self {
        use AbsoluteValue as V;
        match self {
            V::Continuous(v) => {
                let value = round_to_nearest_discrete_value(control_type, v);
                Self::Continuous(value)
            }
            V::Discrete(f) => Self::Discrete(f),
        }
    }

    pub fn has_same_effect_as(self, other: AbsoluteValue) -> bool {
        if let (AbsoluteValue::Discrete(f1), AbsoluteValue::Discrete(f2)) = (self, other) {
            f1.actual() == f2.actual()
        } else {
            // We do an exact comparison here for the moment (no BASE_EPSILON tolerance).
            // Reasoning: We don't know the target epsilon. It's very unlikely but maybe the
            // target cares about minimal differences and then not hitting the target would be
            // bad. Better hit it redundantly instead of omitting a hit that would have made a
            // difference.
            self.to_unit_value() == other.to_unit_value()
        }
    }

    pub fn calc_distance_from(self, rhs: Self) -> Self {
        use AbsoluteValue as V;
        match (self, rhs) {
            (V::Discrete(f1), V::Discrete(f2)) => {
                let distance = (f2.actual() as i32 - f1.actual() as i32).unsigned_abs();
                Self::Discrete(Fraction::new_max(distance))
            }
            _ => {
                let distance = self.to_unit_value().calc_distance_from(rhs.to_unit_value());
                Self::Continuous(distance)
            }
        }
    }

    pub fn is_greater_than(&self, continuous_jump_max: UnitValue, discrete_jump_max: u32) -> bool {
        use AbsoluteValue as V;
        match self {
            V::Continuous(d) => d.get() > continuous_jump_max.get() + BASE_EPSILON,
            V::Discrete(d) => d.actual() > discrete_jump_max,
        }
    }

    pub fn is_lower_than(&self, continuous_jump_min: UnitValue, discrete_jump_min: u32) -> bool {
        use AbsoluteValue as V;
        match self {
            V::Continuous(d) => d.get() + BASE_EPSILON < continuous_jump_min.get(),
            V::Discrete(d) => d.actual() < discrete_jump_min,
        }
    }
}

impl Default for AbsoluteValue {
    fn default() -> Self {
        Self::Continuous(Default::default())
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub enum Increment {
    Continuous(UnitIncrement),
    Discrete(DiscreteIncrement),
}

impl Increment {
    pub fn is_positive(&self) -> bool {
        match self {
            Increment::Continuous(i) => i.is_positive(),
            Increment::Discrete(i) => i.is_positive(),
        }
    }

    /// Returns a unit increment.
    ///
    /// For continuous increments, this just returns the contained value.
    ///
    /// For discrete increments, the atomic unit value is used to convert the integer into
    /// a unit increment. Return `None` if the result would be zero (non-increment).
    pub fn to_unit_increment(&self, atomic_unit_value: UnitValue) -> Option<UnitIncrement> {
        match self {
            Increment::Continuous(i) => Some(*i),
            Increment::Discrete(i) => i.to_unit_increment(atomic_unit_value),
        }
    }

    /// Returns a discrete increment.
    ///
    /// For discrete increments, this just returns the contained value.
    ///
    /// For continuous increments, this returns a +1 or -1 depending on the direction of the
    /// increment. The actual amount is ignored.
    pub fn to_discrete_increment(&self) -> DiscreteIncrement {
        match self {
            Increment::Continuous(i) => i.to_discrete_increment(),
            Increment::Discrete(i) => *i,
        }
    }

    pub fn inverse(&self) -> Increment {
        match self {
            Increment::Continuous(i) => Increment::Continuous(i.inverse()),
            Increment::Discrete(i) => Increment::Discrete(i.inverse()),
        }
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
            continuous.denormalize(&continuous_interval, &discrete_interval, true, Some(500));
        let discrete_normalized =
            discrete.denormalize(&continuous_interval, &discrete_interval, true, Some(500));
        // Then
        assert_abs_diff_eq!(
            continuous_normalized.to_unit_value().get(),
            0.4307086614173229,
            epsilon = BASE_EPSILON
        );
        assert_eq!(
            discrete_normalized,
            AbsoluteValue::Discrete(Fraction::new(205, 500))
        );
    }
}

fn round_to_nearest_discrete_value(
    control_type: ControlType,
    approximate_control_value: UnitValue,
) -> UnitValue {
    // round() is the right choice here vs. floor() because we don't want slight numerical
    // inaccuracies lead to surprising jumps
    use ControlType as T;
    let step_size = match control_type {
        T::AbsoluteContinuousRoundable { rounding_step_size } => rounding_step_size,
        T::AbsoluteDiscrete {
            atomic_step_size, ..
        } => atomic_step_size,
        T::AbsoluteContinuousRetriggerable
        | T::AbsoluteContinuous
        | T::Relative
        | T::VirtualMulti
        | T::VirtualButton => {
            return approximate_control_value;
        }
    };
    approximate_control_value.snap_to_grid_by_interval_size(step_size)
}

pub struct EnhancedTransformationOutput<T> {
    pub produced_kind: ControlValueKind,
    pub value: Option<T>,
    pub instruction: Option<TransformationInstruction>,
}
