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
    ) -> Result<f64, &'static str>;

    fn wants_to_be_polled(&self) -> bool;

    fn transform_continuous(
        &self,
        input: TransformationInput<UnitValue>,
        output_value: UnitValue,
        additional_input: Self::AdditionalInput,
    ) -> Result<UnitValue, &'static str> {
        let res = self.transform(input.map(|v| v.get()), output_value.get(), additional_input)?;
        Ok(UnitValue::new_clamped(res))
    }

    fn transform_discrete(
        &self,
        input: TransformationInput<Fraction>,
        output_value: Fraction,
        additional_input: Self::AdditionalInput,
    ) -> Result<Fraction, &'static str> {
        let res = self.transform(
            input.map(|v| v.actual() as _),
            output_value.actual() as _,
            additional_input,
        )?;
        let actual = res.round() as _;
        let fraction = Fraction::new(actual, std::cmp::max(input.value.max_val(), actual));
        Ok(fraction)
    }
}

pub struct TransformationInput<T> {
    pub value: T,
    pub meta_data: TransformationInputMetaData,
}

impl<T> TransformationInput<T> {
    pub fn new(value: T, meta_data: TransformationInputMetaData) -> Self {
        Self { value, meta_data }
    }
}

#[derive(Copy, Clone)]
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
