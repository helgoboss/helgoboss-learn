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
pub enum FireMode {
    #[cfg_attr(feature = "serde", serde(rename = "release"))]
    #[display(fmt = "When button released (if Min > 0 ms)")]
    WhenButtonReleased,
    #[cfg_attr(feature = "serde", serde(rename = "timeout"))]
    #[display(fmt = "After timeout")]
    AfterTimeout,
    #[cfg_attr(feature = "serde", serde(rename = "turbo"))]
    #[display(fmt = "After timeout, keep firing (turbo)")]
    AfterTimeoutKeepFiring,
}

impl FireMode {
    pub fn wants_to_be_polled(self) -> bool {
        use FireMode::*;
        matches!(self, AfterTimeout | AfterTimeoutKeepFiring)
    }
}

impl Default for FireMode {
    fn default() -> Self {
        Self::WhenButtonReleased
    }
}
