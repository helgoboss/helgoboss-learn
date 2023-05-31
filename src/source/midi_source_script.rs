use crate::{FeedbackValue, MidiSourceAddress, RawMidiEvents};
use std::borrow::Cow;

pub trait MidiSourceScript {
    /// Returns raw MIDI bytes.
    fn execute(
        &self,
        input_value: FeedbackValue,
    ) -> Result<MidiSourceScriptOutcome, Cow<'static, str>>;
}

pub struct MidiSourceScriptOutcome {
    pub address: Option<MidiSourceAddress>,
    pub events: RawMidiEvents,
}
