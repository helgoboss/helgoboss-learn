use crate::{FeedbackValue, MidiSourceScript, MidiSourceScriptOutcome};

pub struct TestMidiSourceScript;

impl MidiSourceScript for TestMidiSourceScript {
    fn execute(
        &self,
        _input_value: FeedbackValue,
    ) -> Result<MidiSourceScriptOutcome, &'static str> {
        unimplemented!()
    }
}
