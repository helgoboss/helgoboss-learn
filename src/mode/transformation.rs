use crate::UnitValue;

/// Represents an arbitrary transformation from one unit value into another one, intended to be
/// implemented by using some form of expression language.
// TODO Make this a trait. Transformation engines should be pluggable.
#[derive(Clone, Debug)]
pub struct Transformation {}

impl Transformation {
    /// Applies the transformation. Should execute fast. If you use an expression or scripting
    /// language, make sure that you compile the expression before-hand.
    pub fn transform(&self, _input_value: UnitValue) -> Result<UnitValue, ()> {
        todo!()
    }
}
