use crate::{ControlValue, Target};

mod absolute;
pub use absolute::*;
mod relative;
pub use relative::*;
mod toggle;
pub use toggle::*;

#[cfg(test)]
mod test_util;

#[derive(Clone, Debug)]
pub enum Mode {
    Absolute(AbsoluteModeData),
    Relative(RelativeModeData),
    Toggle(ToggleModeData),
}

impl Mode {
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
