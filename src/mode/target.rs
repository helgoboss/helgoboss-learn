use crate::UnitValue;

/// When interpreting target value, make only 4 fractional digits matter.
///
/// If we don't do this and target min == target max, even the slightest imprecision of the actual
/// target value (which in practice often occurs with FX parameters not taking exactly the desired
/// value) could result in a totally different feedback value. Maybe it would be better to determine
/// the epsilon dependent on the source precision (e.g. 1.0/128.0 in case of short MIDI messages)
/// but right now this should suffice to solve the immediate problem.  
pub const FEEDBACK_EPSILON: f64 = 0.00001;

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
