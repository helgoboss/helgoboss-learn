use crate::UnitValue;

#[derive(Clone, Debug)]
pub struct Transformation {}

impl Transformation {
    pub fn transform(&self, _input_value: UnitValue) -> Result<UnitValue, ()> {
        todo!()
    }
}
