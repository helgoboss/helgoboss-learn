use crate::UnitValue;

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ControlType {
    /// Targets which don't have a step size.
    AbsoluteContinuous,
    /// Imagine a "tempo" target: Musical tempo is continuous in nature and still you might want to
    /// offer the possibility to round on fraction-less bpm values. Discrete and continuous at the
    /// same time.
    AbsoluteContinuousRoundable { rounding_step_size: UnitValue },
    /// Targets which have a grid of discrete values.
    AbsoluteDiscrete { atomic_step_size: UnitValue },
    /// If target wants to be controlled via relative increments.
    Relative,
    /// For virtual targets that don't know about the nature of the real target.
    Virtual,
}

impl ControlType {
    pub fn is_relative(&self) -> bool {
        *self == ControlType::Relative
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
    fn current_value(&self) -> UnitValue;

    fn control_type(&self) -> ControlType;
}
