/// Incoming value which might be used to control something
#[derive(Clone, PartialEq, Debug)]
pub enum OscSourceValue {
    Plain(rosc::OscMessage),
}
