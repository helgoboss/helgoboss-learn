mod target;
pub use target::*;
mod absolute;
pub use absolute::*;
mod relative;
pub use relative::*;
mod toggle;
pub use toggle::*;
mod transformation;
use crate::{ControlValue, UnitValue};
pub use transformation::*;

#[cfg(test)]
mod test_util;

// TODO This enum is not so helpful. It just delegates and encourages a uniform API (a trait would
//  be more helpful for the latter) ... but I think client code should decide how to aggregate
//  different modes. So maybe remove this.
/// Different modes for interpreting and transforming control or feedback values.
#[derive(Clone, Debug)]
pub enum Mode<T: Transformation> {
    Absolute(AbsoluteModeData<T>),
    Relative(RelativeModeData),
    Toggle(ToggleModeData),
}

impl<T: Transformation> Mode<T> {
    /// Takes a control value, interprets and transforms it and maybe returns an appropriate
    /// target value which should be sent to the target.
    pub fn control(
        &self,
        control_value: ControlValue,
        target: &impl Target,
    ) -> Option<ControlValue> {
        match self {
            Mode::Absolute(data) => data.control(control_value, target),
            Mode::Relative(data) => data.control(control_value, target),
            Mode::Toggle(data) => data.control(control_value, target),
        }
    }

    /// Takes a target value, interprets and transforms it and maybe returns an appropriate
    /// source value that should be sent to the source.
    pub fn feedback(&self, target_value: UnitValue) -> Option<UnitValue> {
        use Mode::*;
        match self {
            Absolute(data) => data.feedback(target_value),
            Relative(data) => data.feedback(target_value),
            Toggle(data) => data.feedback(target_value),
        }
    }
}
