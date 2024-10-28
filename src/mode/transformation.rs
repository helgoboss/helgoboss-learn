use crate::{
    ControlValue, ControlValueKind, DiscreteIncrement, Fraction, UnitIncrement, UnitValue,
};
use std::time::Duration;

/// Represents an arbitrary transformation from one unit value into another one, intended to be
/// implemented by using some form of expression language.
pub trait Transformation {
    type AdditionalInput: Default;

    /// Applies the transformation.
    ///
    /// Should execute fast. If you use an expression or scripting language, make sure that you
    /// compile the expression beforehand.
    fn transform(
        &self,
        input: TransformationInput<Self::AdditionalInput>,
    ) -> Result<TransformationOutput, &'static str>;

    fn wants_to_be_polled(&self) -> bool;
}

#[derive(Default)]
pub struct TransformationInput<A> {
    pub event: TransformationInputEvent,
    pub context: TransformationInputContext,
    /// Consumers can pass through more stuff to the transformation script if they want.
    pub additional_input: A,
}

#[derive(Default)]
pub struct TransformationInputEvent {
    pub input_value: f64,
    pub timestamp: Duration,
}

#[derive(Default)]
pub struct TransformationInputContext {
    pub output_value: f64,
    /// Duration since last interaction. For modulations/transitions only.
    pub rel_time: Duration,
}

/// Output of the transformation.
///
/// If both `value` and `instruction` are `None`, it means that the target shouldn't be invoked:
///
/// - Usually, each repeated invocation always results in a target invocation (unless the target is
///   not retriggerable and already has the desired value).
/// - Sometimes this is not desired. In this case, one can return `none`, in which case the target
///   will not be touched.
/// - Good for transitions that are not continuous, especially if other mappings want to control
///   the parameter as well from time to time.
#[derive(Copy, Clone, Debug)]
pub struct TransformationOutput {
    /// The kind of control values which this transformation produces.
    ///
    /// This should always be available, as it might be queried statically for GUI purposes.
    pub produced_kind: ControlValueKind,
    pub value: Option<f64>,
    pub instruction: Option<TransformationInstruction>,
}

impl TransformationOutput {
    pub fn extract_control_value(&self, in_discrete_max: Option<u32>) -> Option<ControlValue> {
        let raw = self.value?;
        let cv = match self.produced_kind {
            ControlValueKind::AbsoluteContinuous => {
                ControlValue::AbsoluteContinuous(UnitValue::new_clamped(raw))
            }
            ControlValueKind::RelativeDiscrete => {
                let inc = raw.round() as i32;
                ControlValue::RelativeDiscrete(DiscreteIncrement::new_checked(inc)?)
            }
            ControlValueKind::RelativeContinuous => {
                ControlValue::RelativeContinuous(UnitIncrement::new_clamped(raw))
            }
            ControlValueKind::AbsoluteDiscrete => {
                let actual = raw.round() as _;
                let max = match in_discrete_max {
                    None => actual,
                    Some(max) => std::cmp::max(max, actual),
                };
                ControlValue::AbsoluteDiscrete(Fraction::new(actual, max))
            }
        };
        Some(cv)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum TransformationInstruction {
    /// This stops repeated invocation of the formula until the mapping is triggered again.
    ///
    /// - Good for building transitions with a defined end.
    /// - Stopping the invocation at some point is also important if the same parameter shall be
    ///   controlled by other mappings as well. If multiple mappings continuously change the target
    ///   parameter, only the last one wins.
    Stop,
}
