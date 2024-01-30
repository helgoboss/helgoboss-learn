use crate::{FeedbackValue, MidiSourceScript, MidiSourceScriptOutcome};
use std::borrow::Cow;

pub struct TestMidiSourceScript;

impl MidiSourceScript<'_> for TestMidiSourceScript {
    type AdditionalInput = ();

    fn execute(
        &self,
        _input_value: FeedbackValue,
        _additional_input: (),
    ) -> Result<MidiSourceScriptOutcome, Cow<'static, str>> {
        unimplemented!()
    }
}
