use crate::{AbsoluteValue, MidiSourceScript, RawMidiEvents};

pub struct TestMidiSourceScript;

impl MidiSourceScript for TestMidiSourceScript {
    fn execute(&self, _input_value: AbsoluteValue) -> Result<RawMidiEvents, &'static str> {
        unimplemented!()
    }
}
