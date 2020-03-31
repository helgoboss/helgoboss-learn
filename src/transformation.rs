use crate::UnitValue;

#[derive(Clone, Debug)]
pub struct Transformation {}

impl Transformation {
    pub fn transform(&self, input_value: UnitValue) -> Result<UnitValue, ()> {
        unimplemented!()
    }
}
