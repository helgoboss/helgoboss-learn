use crate::{Fraction, UnitValue};
use std::time::Duration;

/// Represents an arbitrary transformation from one unit value into another one, intended to be
/// implemented by using some form of expression language.
pub trait Transformation {
    type AdditionalInput: Default;

    /// Applies the transformation.
    ///
    /// Should execute fast. If you use an expression or scripting language, make sure that you
    /// compile the expression before-hand.
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
    ) -> Result<TransformationOutput<UnitValue>, &'static str> {
        let res = self.transform(input.map(|v| v.get()), output_value.get(), additional_input)?;
        Ok(res.map(UnitValue::new_clamped))
    }

    fn transform_discrete(
        &self,
        input: TransformationInput<Fraction>,
        output_value: Fraction,
        additional_input: Self::AdditionalInput,
    ) -> Result<TransformationOutput<Fraction>, &'static str> {
        let res = self.transform(
            input.map(|v| v.actual() as _),
            output_value.actual() as _,
            additional_input,
        )?;
        let fraction = res.map(|v| {
            let actual = v.round() as _;
            Fraction::new(actual, std::cmp::max(input.value.max_val(), actual))
        });
        Ok(fraction)
    }
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

#[derive(Copy, Clone)]
pub enum TransformationOutput<T> {
    /// This stops repeated invocation of the formula until the mapping is triggered again.
    ///
    /// - Good for building transitions with a defined end.
    /// - Stopping the invocation at some point is also important if the same parameter shall be
    ///   controlled by other mappings as well. If multiple mappings continuously change the target
    ///   parameter, only the last one wins.
    Stop,
    /// Does nothing (doesn't invoke the target).
    ///
    /// - Usually, each repeated invocation always results in a target invocation (unless the target is
    ///   not retriggerable and already has the desired value).
    /// - Sometimes this is not desired. In this case, one can return `none`, in which case the target
    ///   will not be touched.
    /// - Good for transitions that are not continuous, especially if other mappings want to control
    ///   the parameter as well from time to time.
    None,
    /// A control value.
    Control(T),
    /// A combination of Control and Stop.
    ControlAndStop(T),
}

impl<T: Copy> TransformationOutput<T> {
    pub fn map<R>(&self, f: impl FnOnce(T) -> R) -> TransformationOutput<R> {
        match self {
            TransformationOutput::Stop => TransformationOutput::Stop,
            TransformationOutput::None => TransformationOutput::None,
            TransformationOutput::Control(v) => TransformationOutput::Control(f(*v)),
            TransformationOutput::ControlAndStop(v) => TransformationOutput::ControlAndStop(f(*v)),
        }
    }

    pub fn value(&self) -> Option<T> {
        match self {
            TransformationOutput::Control(v) | TransformationOutput::ControlAndStop(v) => Some(*v),
            TransformationOutput::Stop | TransformationOutput::None => None,
        }
    }
}
