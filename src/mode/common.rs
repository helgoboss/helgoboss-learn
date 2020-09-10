use derive_more::Display;
use enum_iterator::IntoEnumIterator;
use num_enum::{IntoPrimitive, TryFromPrimitive};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

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
    #[cfg_attr(feature = "serde", serde(rename = "min-or-max"))]
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
