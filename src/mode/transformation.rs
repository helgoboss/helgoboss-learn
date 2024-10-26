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
        input: TransformationInput<f64>,
        output_value: f64,
        additional_input: Self::AdditionalInput,
    ) -> Result<TransformationOutput<f64>, &'static str>;

    fn wants_to_be_polled(&self) -> bool;

    fn transform_continuous(
        &self,
        input: TransformationInput<UnitValue>,
        output_value: UnitValue,
        additional_input: Self::AdditionalInput,
    ) -> Result<TransformationOutput<ControlValue>, &'static str> {
        let out = self.transform(input.map(|v| v.get()), output_value.get(), additional_input)?;
        let out = TransformationOutput {
            produced_kind: out.produced_kind,
            value: out
                .value
                .and_then(|raw| convert_f64_to_control_value(raw, out.produced_kind, None)),
            instruction: out.instruction,
        };
        Ok(out)
    }

    // Not currently used as discrete control not yet unlocked.
    fn transform_discrete(
        &self,
        input: TransformationInput<Fraction>,
        output_value: Fraction,
        additional_input: Self::AdditionalInput,
    ) -> Result<TransformationOutput<ControlValue>, &'static str> {
        let out = self.transform(
            input.map(|v| v.actual() as _),
            output_value.actual() as _,
            additional_input,
        )?;
        let out = TransformationOutput {
            produced_kind: out.produced_kind,
            value: out.value.and_then(|raw| {
                convert_f64_to_control_value(raw, out.produced_kind, Some(input.value.max_val()))
            }),
            instruction: out.instruction,
        };
        Ok(out)
    }
}

fn convert_f64_to_control_value(
    raw: f64,
    kind: ControlValueKind,
    in_discrete_max: Option<u32>,
) -> Option<ControlValue> {
    let cv = match kind {
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

#[derive(Default)]
pub struct TransformationInput<T> {
    pub value: T,
    pub meta_data: TransformationInputMetaData,
}

impl<T> TransformationInput<T> {
    pub fn new(value: T, meta_data: TransformationInputMetaData) -> Self {
        Self { value, meta_data }
    }
}

#[derive(Copy, Clone, Default)]
pub struct TransformationInputMetaData {
    pub rel_time: Duration,
}

impl<T: Copy> TransformationInput<T> {
    fn map<R>(&self, f: impl FnOnce(T) -> R) -> TransformationInput<R> {
        TransformationInput {
            value: f(self.value),
            meta_data: self.meta_data,
        }
    }
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
pub struct TransformationOutput<T> {
    /// The kind of control values which this transformation produces.
    ///
    /// This should always be available, as it might be queried statically for GUI purposes.
    pub produced_kind: ControlValueKind,
    pub value: Option<T>,
    pub instruction: Option<TransformationInstruction>,
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
