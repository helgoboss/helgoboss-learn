use crate::UnitValue;

/// Represents an arbitrary transformation from one unit value into another one, intended to be
/// implemented by using some form of expression language.
pub trait Transformation {
    /// Applies the transformation.
    ///
    /// Should execute fast.If you use an expression or scripting language, make sure that you
    /// compile the expression before-hand.
    fn transform(&self, input_value: UnitValue, output_value: UnitValue) -> Result<UnitValue, ()>;
}
