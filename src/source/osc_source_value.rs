/// Incoming value which might be used to control something
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum OscSourceValue<'a> {
    Plain(&'a rosc::OscMessage),
}
