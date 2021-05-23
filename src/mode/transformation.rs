use crate::{Fraction, UnitValue};

/// Represents an arbitrary transformation from one unit value into another one, intended to be
/// implemented by using some form of expression language.
pub trait Transformation {
    /// Applies the transformation.
    ///
    /// Should execute fast.If you use an expression or scripting language, make sure that you
    /// compile the expression before-hand.
    fn transform(&self, input_value: f64, output_value: f64) -> Result<f64, &'static str>;

    fn transform_continuous(
        &self,
        input_value: UnitValue,
        output_value: UnitValue,
    ) -> Result<UnitValue, &'static str> {
        let res = self.transform(input_value.get(), output_value.get())?;
        Ok(UnitValue::new_clamped(res))
    }

    fn transform_discrete(
        &self,
        input_value: Fraction,
        output_value: Fraction,
    ) -> Result<Fraction, &'static str> {
        let res = self.transform(input_value.actual() as _, output_value.actual() as _)?;
        let actual = res.round() as _;
        Ok(Fraction::new(
            actual,
            std::cmp::max(input_value.max_val(), actual),
        ))
    }
}
