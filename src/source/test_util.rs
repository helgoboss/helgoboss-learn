use crate::{FeedbackValue, MidiSourceScript, MidiSourceScriptOutcome};
use std::borrow::Cow;

pub struct TestMidiSourceScript;

impl MidiSourceScript for TestMidiSourceScript {
    fn execute(
        &self,
        _input_value: FeedbackValue,
    ) -> Result<MidiSourceScriptOutcome, Cow<'static, str>> {
        unimplemented!()
    }
}
