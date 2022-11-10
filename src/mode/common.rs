use crate::{AbsoluteValue, Increment, Interval, IntervalMatchResult, MinIsMaxBehavior, UnitValue};
use derive_more::Display;
use enum_iterator::IntoEnumIterator;
use num_enum::{IntoPrimitive, TryFromPrimitive};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// This epsilon is used in helgoboss-learn at some places to make floating point comparison
/// more tolerant. This is the same epsilon used in JSFX/EEL.   
pub const BASE_EPSILON: f64 = 0.00001;

/// Determines how out-of-range source (control) or target (feedback) values are handled.
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    Debug,
    IntoEnumIterator,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(usize)]
pub enum OutOfRangeBehavior {
    /// Yields range minimum if lower than range minimum and range maximum if greater.
    #[cfg_attr(feature = "serde", serde(rename = "minOrMax"))]
    #[display(fmt = "Min or max")]
    MinOrMax,
    /// Yields range minimum if out-of-range.
    #[cfg_attr(feature = "serde", serde(rename = "min"))]
    #[display(fmt = "Min")]
    Min,
    /// Totally ignores out-of-range values.
    #[cfg_attr(feature = "serde", serde(rename = "ignore"))]
    #[display(fmt = "Ignore")]
    Ignore,
}

impl Default for OutOfRangeBehavior {
    fn default() -> Self {
        OutOfRangeBehavior::MinOrMax
    }
}

impl OutOfRangeBehavior {
    pub fn process(
        &self,
        control_value: AbsoluteValue,
        interval_match_result: IntervalMatchResult,
        continuous_interval: &Interval<UnitValue>,
        discrete_interval: &Interval<u32>,
    ) -> Option<(AbsoluteValue, MinIsMaxBehavior)> {
        use OutOfRangeBehavior::*;
        match self {
            MinOrMax => {
                if interval_match_result == IntervalMatchResult::Lower {
                    Some((
                        control_value.select_appropriate_interval_min(
                            continuous_interval,
                            discrete_interval,
                        ),
                        MinIsMaxBehavior::PreferZero,
                    ))
                } else {
                    Some((
                        control_value.select_appropriate_interval_max(
                            continuous_interval,
                            discrete_interval,
                        ),
                        MinIsMaxBehavior::PreferOne,
                    ))
                }
            }
            Min => Some((
                control_value
                    .select_appropriate_interval_min(continuous_interval, discrete_interval),
                MinIsMaxBehavior::PreferZero,
            )),
            Ignore => None,
        }
    }
}

#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    Debug,
    IntoEnumIterator,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(usize)]
pub enum ButtonUsage {
    #[cfg_attr(feature = "serde", serde(rename = "both"))]
    #[display(fmt = "Press & release")]
    Both,
    #[cfg_attr(feature = "serde", serde(rename = "press-only"))]
    #[display(fmt = "Press only")]
    PressOnly,
    #[cfg_attr(feature = "serde", serde(rename = "release-only"))]
    #[display(fmt = "Release only")]
    ReleaseOnly,
}

impl Default for ButtonUsage {
    fn default() -> Self {
        ButtonUsage::Both
    }
}

#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    Debug,
    IntoEnumIterator,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(usize)]
pub enum EncoderUsage {
    #[cfg_attr(feature = "serde", serde(rename = "both"))]
    #[display(fmt = "Increment & decrement")]
    Both,
    #[cfg_attr(feature = "serde", serde(rename = "increment-only"))]
    #[display(fmt = "Increment only")]
    IncrementOnly,
    #[cfg_attr(feature = "serde", serde(rename = "decrement-only"))]
    #[display(fmt = "Decrement only")]
    DecrementOnly,
}

impl EncoderUsage {
    pub fn matches(&self, i: Increment) -> bool {
        match self {
            EncoderUsage::IncrementOnly if !i.is_positive() => false,
            EncoderUsage::DecrementOnly if i.is_positive() => false,
            _ => true,
        }
    }
}

impl Default for EncoderUsage {
    fn default() -> Self {
        EncoderUsage::Both
    }
}

#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    Debug,
    IntoEnumIterator,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(usize)]
pub enum FireMode {
    #[cfg_attr(feature = "serde", serde(rename = "release"))]
    #[display(fmt = "Fire on press (or release if > 0 ms)")]
    Normal,
    #[cfg_attr(feature = "serde", serde(rename = "timeout"))]
    #[display(fmt = "Fire after timeout")]
    AfterTimeout,
    #[cfg_attr(feature = "serde", serde(rename = "turbo"))]
    #[display(fmt = "Fire after timeout, keep firing (turbo)")]
    AfterTimeoutKeepFiring,
    #[cfg_attr(feature = "serde", serde(rename = "single"))]
    #[display(fmt = "Fire after single press")]
    OnSinglePress,
    #[cfg_attr(feature = "serde", serde(rename = "double"))]
    #[display(fmt = "Fire on double press")]
    OnDoublePress,
}

impl Default for FireMode {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    Debug,
    IntoEnumIterator,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(usize)]
pub enum TakeoverMode {
    #[cfg_attr(feature = "serde", serde(rename = "normal"))]
    #[display(fmt = "Normal (jumps possible)")]
    Normal,
    #[cfg_attr(feature = "serde", serde(rename = "pickup-tolerant"))]
    #[display(fmt = "Pick up (tolerant)")]
    PickupTolerant,
    #[cfg_attr(feature = "serde", serde(rename = "pickup"))]
    #[display(fmt = "Pick up")]
    Pickup,
    #[cfg_attr(feature = "serde", serde(rename = "longTimeNoSee"))]
    #[display(fmt = "Long time no see")]
    LongTimeNoSee,
    #[cfg_attr(feature = "serde", serde(rename = "parallel"))]
    #[display(fmt = "Parallel")]
    Parallel,
    #[cfg_attr(feature = "serde", serde(rename = "valueScaling"))]
    #[display(fmt = "Catch up")]
    CatchUp,
}

impl TakeoverMode {
    pub fn prevents_jumps(&self) -> bool {
        *self != Self::Normal
    }
}

impl Default for TakeoverMode {
    fn default() -> Self {
        Self::Pickup
    }
}

#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    Debug,
    IntoEnumIterator,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(usize)]
pub enum GroupInteraction {
    #[cfg_attr(feature = "serde", serde(rename = "none"))]
    #[display(fmt = "None")]
    None,
    #[cfg_attr(feature = "serde", serde(rename = "same-control"))]
    #[display(fmt = "Same control")]
    SameControl,
    #[cfg_attr(feature = "serde", serde(rename = "same-target-value"))]
    #[display(fmt = "Same target value")]
    SameTargetValue,
    #[cfg_attr(feature = "serde", serde(rename = "inverse-control"))]
    #[display(fmt = "Inverse control")]
    InverseControl,
    #[cfg_attr(feature = "serde", serde(rename = "inverse-target-value"))]
    #[display(fmt = "Inverse target value")]
    InverseTargetValue,
    #[cfg_attr(feature = "serde", serde(rename = "inverse-target-value-on-only"))]
    #[display(fmt = "Inverse target value (on only)")]
    InverseTargetValueOnOnly,
}

impl Default for GroupInteraction {
    fn default() -> Self {
        Self::None
    }
}

impl GroupInteraction {
    pub fn is_target_based(&self) -> bool {
        use GroupInteraction::*;
        matches!(self, SameTargetValue | InverseTargetValue)
    }

    pub fn is_inverse(self) -> bool {
        use GroupInteraction::*;
        matches!(
            self,
            InverseControl | InverseTargetValue | InverseTargetValueOnOnly
        )
    }
}
