use crate::{AbsoluteValue, Increment, Interval, IntervalMatchResult, MinIsMaxBehavior, UnitValue};
use derive_more::Display;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use strum::EnumIter;

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
    Default,
    EnumIter,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
    Serialize,
    Deserialize,
)]
#[repr(usize)]
pub enum OutOfRangeBehavior {
    /// Yields range minimum if lower than range minimum and range maximum if greater.
    #[default]
    #[serde(rename = "minOrMax")]
    #[display(fmt = "Min or max")]
    MinOrMax,
    /// Yields range minimum if out-of-range.
    #[serde(rename = "min")]
    #[display(fmt = "Min")]
    Min,
    /// Totally ignores out-of-range values.
    #[serde(rename = "ignore")]
    #[display(fmt = "Ignore")]
    Ignore,
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
    Default,
    EnumIter,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
    Serialize,
    Deserialize,
)]
#[repr(usize)]
pub enum ButtonUsage {
    #[default]
    #[serde(rename = "both")]
    #[display(fmt = "Press & release")]
    Both,
    #[serde(rename = "press-only")]
    #[display(fmt = "Press only")]
    PressOnly,
    #[serde(rename = "release-only")]
    #[display(fmt = "Release only")]
    ReleaseOnly,
}

impl ButtonUsage {
    pub fn should_ignore(&self, value: AbsoluteValue) -> bool {
        match self {
            ButtonUsage::PressOnly if value.is_zero() => true,
            ButtonUsage::ReleaseOnly if !value.is_zero() => true,
            _ => false,
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
    Default,
    EnumIter,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
    Serialize,
    Deserialize,
)]
#[repr(usize)]
pub enum EncoderUsage {
    #[default]
    #[serde(rename = "both")]
    #[display(fmt = "Increment & decrement")]
    Both,
    #[serde(rename = "increment-only")]
    #[display(fmt = "Increment only")]
    IncrementOnly,
    #[serde(rename = "decrement-only")]
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

#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    Debug,
    EnumIter,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
    Serialize,
    Deserialize,
)]
#[repr(usize)]
pub enum FireMode {
    #[serde(rename = "release")]
    #[display(fmt = "Fire on press (or release if > 0 ms)")]
    Normal,
    #[serde(rename = "timeout")]
    #[display(fmt = "Fire after timeout")]
    AfterTimeout,
    #[serde(rename = "turbo")]
    #[display(fmt = "Fire after timeout, keep firing (turbo)")]
    AfterTimeoutKeepFiring,
    #[serde(rename = "single")]
    #[display(fmt = "Fire after single press")]
    OnSinglePress,
    #[serde(rename = "double")]
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
    EnumIter,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
    Serialize,
    Deserialize,
)]
#[repr(usize)]
pub enum TakeoverMode {
    #[serde(rename = "off")]
    #[display(fmt = "Off (may cause jumps)")]
    Off,
    #[serde(rename = "pickup")]
    #[display(fmt = "Pick up")]
    Pickup,
    #[serde(rename = "pickup-tolerant")]
    #[display(fmt = "Pick up (tolerant)")]
    PickupTolerant,
    #[serde(rename = "longTimeNoSee")]
    #[display(fmt = "Long time no see")]
    LongTimeNoSee,
    #[serde(rename = "parallel")]
    #[display(fmt = "Parallel")]
    Parallel,
    #[serde(rename = "valueScaling")]
    #[display(fmt = "Catch up")]
    CatchUp,
}

impl TakeoverMode {
    pub fn prevents_jumps(&self) -> bool {
        *self != Self::Off
    }
}

impl Default for TakeoverMode {
    fn default() -> Self {
        Self::Off
    }
}

#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    Debug,
    EnumIter,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
    Serialize,
    Deserialize,
)]
#[repr(usize)]
pub enum GroupInteraction {
    #[serde(rename = "none")]
    #[display(fmt = "None")]
    None,
    #[serde(rename = "same-control")]
    #[display(fmt = "Same control")]
    SameControl,
    #[serde(rename = "same-target-value")]
    #[display(fmt = "Same target value")]
    SameTargetValue,
    #[serde(rename = "inverse-control")]
    #[display(fmt = "Inverse control")]
    InverseControl,
    #[serde(rename = "inverse-target-value")]
    #[display(fmt = "Inverse target value")]
    InverseTargetValue,
    #[serde(rename = "inverse-target-value-on-only")]
    #[display(fmt = "Inverse target value (on only)")]
    InverseTargetValueOnOnly,
    #[serde(rename = "inverse-target-value-off-only")]
    #[display(fmt = "Inverse target value (off only)")]
    InverseTargetValueOffOnly,
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
            InverseControl
                | InverseTargetValue
                | InverseTargetValueOnOnly
                | InverseTargetValueOffOnly
        )
    }
}
