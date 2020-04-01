mod target;
pub use target::*;
mod absolute;
pub use absolute::*;
mod relative;
pub use relative::*;
mod toggle;
pub use toggle::*;
mod transformation;
use crate::ControlValue;
pub use transformation::*;

#[cfg(test)]
mod test_util;

#[derive(Clone, Debug)]
pub enum Mode<T: Transformation> {
    Absolute(AbsoluteModeData<T>),
    Relative(RelativeModeData),
    Toggle(ToggleModeData),
}

impl<T: Transformation> Mode<T> {
    pub fn process(
        &self,
        control_value: ControlValue,
        target: &impl Target,
    ) -> Option<ControlValue> {
        use ControlValue::*;
        match self {
            Mode::Absolute(data) => match control_value {
                Absolute(v) => data.process(v, target),
                Relative(_) => None,
            },
            Mode::Relative(data) => data.process(control_value, target),
            Mode::Toggle(data) => match control_value {
                Absolute(v) => data.process(v, target),
                Relative(_) => None,
            },
        }
    }
}
