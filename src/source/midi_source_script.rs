use crate::{FeedbackValue, MidiSourceAddress, RawMidiEvents};

pub trait MidiSourceScript {
    /// Returns raw MIDI bytes.
    fn execute(&self, input_value: FeedbackValue) -> Result<MidiSourceScriptOutcome, &'static str>;
}

pub struct MidiSourceScriptOutcome {
    pub address: Option<MidiSourceAddress>,
    pub events: RawMidiEvents,
}
