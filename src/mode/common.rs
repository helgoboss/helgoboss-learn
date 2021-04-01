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
    WhenButtonReleased,
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
        Self::WhenButtonReleased
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

impl Default for TakeoverMode {
    fn default() -> Self {
        Self::Pickup
    }
}
