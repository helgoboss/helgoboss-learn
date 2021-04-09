use crate::{MidiSourceScript, RawMidiEvent, UnitValue};

pub struct TestMidiSourceScript;

impl MidiSourceScript for TestMidiSourceScript {
    fn execute(&self, _input_value: UnitValue) -> Result<Box<RawMidiEvent>, &'static str> {
        unimplemented!()
    }
}
