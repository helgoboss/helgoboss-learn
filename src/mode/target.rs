use crate::{AbsoluteValue, UnitValue};

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ControlType {
    /// Targets which don't have a step size.
    AbsoluteContinuous,
    /// The only difference to AbsoluteContinuous is that it gets retriggered even it already has
    /// the desired target value.
    AbsoluteContinuousRetriggerable,
    /// Imagine a "tempo" target: Musical tempo is continuous in nature and still you might want to
    /// offer the possibility to round on fraction-less bpm values. Discrete and continuous at the
    /// same time.
    AbsoluteContinuousRoundable { rounding_step_size: UnitValue },
    /// Targets which have a grid of discrete values.
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

    /// Returns a textual feedback value for the given key if the target supports it.
    //
    // Requiring owned strings here makes the API more pleasant and is probably not a big deal
    // performance-wise (feedback strings don't get large). If we want to optimize this in future,
    // don't use Cows. The only real performance win would be to use a writer API.
    // With Cows, we would still need to turn ReaperStr into owned String. With writer API,
    // we could just read borrowed ReaperStr as str and write into the result buffer. However, in
    // practice we don't often get borrowed strings from Reaper anyway.
    // TODO-low Use a formatter API instead of returning owned strings (for this to work, we also
    //  need to adjust the textual feedback expression parsing to take advantage of it).
    fn textual_value(&self, key: TargetPropKey, context: Self::Context) -> Option<String> {
        let _ = key;
        let _ = context;
        None
    }

    fn control_type(&self, context: Self::Context) -> ControlType;
}

pub enum TargetPropKey<'a> {
    Default,
    Custom(&'a str),
}

impl<'a> From<&'a str> for TargetPropKey<'a> {
    fn from(text: &'a str) -> Self {
        match text.trim() {
            "default" => Self::Default,
            trimmed => Self::Custom(trimmed),
        }
    }
}
