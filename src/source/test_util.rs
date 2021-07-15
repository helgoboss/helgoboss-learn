use crate::{AbsoluteValue, MidiSourceScript, RawMidiEvent};

pub struct TestMidiSourceScript;

impl MidiSourceScript for TestMidiSourceScript {
    fn execute(&self, _input_value: AbsoluteValue) -> Result<Box<RawMidiEvent>, &'static str> {
        unimplemented!()
    }
}
