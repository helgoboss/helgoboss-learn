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

    fn control_type(&self) -> ControlType;
}
