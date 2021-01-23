use crate::UnitValue;

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ControlType {
    AbsoluteTrigger,
    AbsoluteSwitch,
    /// Targets which don't have a step size.
    AbsoluteContinuous,
    /// Imagine a "tempo" target: Musical tempo is continuous in nature and still you might want to
    /// offer the possibility to round on fraction-less bpm values. Discrete and continuous at the
    /// same time.
    AbsoluteContinuousRoundable {
        rounding_step_size: UnitValue,
    },
    /// Targets which have a grid of discrete values.
    AbsoluteDiscrete {
        atomic_step_size: UnitValue,
    },
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

    pub fn is_trigger(&self) -> bool {
        matches!(self, ControlType::AbsoluteTrigger)
    }

    pub fn step_size(&self) -> Option<UnitValue> {
        use ControlType::*;
        match self {
            AbsoluteContinuousRoundable { rounding_step_size } => Some(*rounding_step_size),
            AbsoluteDiscrete { atomic_step_size } => Some(*atomic_step_size),
            _ => None,
        }
    }
}

pub trait Target {
    /// Should return the current value of the target.
    ///
    /// Some targets don't have the notion of a current value, e.g. virtual targets (which are just
    /// mediators really). Other targets might momentarily not be able to return a current value.
    /// In such cases, `None` should be returned so that the mode can handle this situation
    /// gracefully. Of course, some mode features won't work without knowing the current value,
    /// but others will still work.
    fn current_value(&self) -> Option<UnitValue>;

    fn control_type(&self) -> ControlType;
}
