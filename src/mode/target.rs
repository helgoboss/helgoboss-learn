use crate::{AbsoluteValue, UnitValue};

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ControlType {
    /// Targets which don't have a step size, targets that have just on/off states and trigger
    /// targets.
    AbsoluteContinuous,
    /// The only difference to AbsoluteContinuous is that it gets retriggered even it already has
    /// the desired target value.
    AbsoluteContinuousRetriggerable,
    /// Imagine a "tempo" target: Musical tempo is continuous in nature and still you might want to
    /// offer the possibility to round on fraction-less bpm values. Discrete and continuous at the
    /// same time.
    ///
    /// In more recent versions, the same can be achieved using target value sequences.
    AbsoluteContinuousRoundable { rounding_step_size: UnitValue },
    /// Targets which have a grid of discrete values (and therefore a step size).
    AbsoluteDiscrete { atomic_step_size: UnitValue },
    /// If target wants to be controlled via relative increments.
    Relative,
    /// For virtual continuous targets (that don't know about the nature of the real target).
    VirtualMulti,
    /// For virtual button targets (that don't know about the nature of the real target).
    VirtualButton,
}

impl ControlType {
    pub fn is_relative(&self) -> bool {
        *self == ControlType::Relative
    }

    pub fn is_retriggerable(&self) -> bool {
        matches!(self, ControlType::AbsoluteContinuousRetriggerable)
    }

    pub fn step_size(&self) -> Option<UnitValue> {
        use ControlType::*;
        match self {
            AbsoluteContinuousRoundable { rounding_step_size } => Some(*rounding_step_size),
            AbsoluteDiscrete { atomic_step_size } => Some(*atomic_step_size),
            _ => None,
        }
    }

    pub fn discrete_count(&self) -> Option<u32> {
        Some(self.discrete_max()? + 1)
    }

    pub fn discrete_max(&self) -> Option<u32> {
        let step_size = self.step_size()?;
        if step_size.is_zero() {
            return None;
        }
        Some((1.0 / step_size.get()).round() as u32)
    }

    pub fn is_virtual(&self) -> bool {
        use ControlType::*;
        matches!(self, VirtualMulti | VirtualButton)
    }
}

pub trait Target<'a> {
    type Context: Copy;

    /// Should return the current value of the target.
    ///
    /// Some targets don't have the notion of a current value, e.g. virtual targets (which are just
    /// mediators really). Other targets might momentarily not be able to return a current value.
    /// In such cases, `None` should be returned so that the mode can handle this situation
    /// gracefully. Of course, some mode features won't work without knowing the current value,
    /// but others will still work.
    fn current_value(&self, context: Self::Context) -> Option<AbsoluteValue>;

    fn control_type(&self, context: Self::Context) -> ControlType;
}

/// Some standardized property keys.
pub mod target_prop_keys {
    /// Short text representing the current target value, including a possible unit.
    ///
    /// This is the default value shown if textual feedback is enabled and the textual feedback
    /// expression is empty. Choose the textual representation that's most likely to be desired.
    /// If there's some name to display, prefer that name over a numeric representation.
    ///
    /// Examples:
    ///
    /// - Track: Volume → "-6.00 dB"
    /// - Track: Mute/unmute → "Mute"
    /// - Project: Navigate within tracks → "Guitar"
    pub const TEXT_VALUE: &str = "text_value";

    /// Non-normalized representing the current target value as a *human-friendly number*
    /// (type: [`crate::NumericValue`]).
    ///
    /// The purpose of this is to allow for more freedom in formatting numerical target values than
    /// when using [`TEXT_VALUE`]. Future versions of ReaLearn might extend textual feedback
    /// expressions in a way so the user can define how exactly the numerical value is presented
    /// (decimal points etc.).
    ///
    /// "Human-readable" also means that if it's a position, then it's really a position number
    /// (one-rooted), not an index number (zero-rooted).
    ///
    /// - Track: Volume → -6.00
    /// - Track: Mute/unmute → 1.0
    /// - Project: Navigate within tracks → 5
    pub const NUMERIC_VALUE: &str = "numeric_value";

    /// Unit of the non-normalized number in human-friendly form.
    ///
    /// - Track: Volume → "dB"
    /// - Track: Mute/unmute → ""
    /// - Project: Navigate within tracks → ""
    pub const NUMERIC_VALUE_UNIT: &str = "numeric_value.unit";

    /// Normalized value in the unit interval. You can think of it as a percentage.
    ///
    /// This value is available for most targets and good if you need a totally uniform
    /// representation of the target value that doesn't differ between target types. By default,
    /// this is formatted as percentage. Future versions of ReaLearn might offer user-defined
    /// formatting. E.g. this will also be the preferred form to format on/off states in a
    /// custom way (where 0% represents "off").
    ///
    /// - Track: Volume → 0.5
    /// - Track: Mute/unmute → 0.0
    /// - Project: Navigate within tracks → 0.7
    pub const NORMALIZED_VALUE: &str = "normalized_value";
}
