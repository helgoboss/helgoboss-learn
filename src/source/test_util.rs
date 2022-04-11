use crate::{FeedbackValue, MidiSourceScript, RawMidiEvents};

pub struct TestMidiSourceScript;

impl MidiSourceScript for TestMidiSourceScript {
    fn execute(&self, _input_value: FeedbackValue) -> Result<RawMidiEvents, &'static str> {
        unimplemented!()
    }
}
