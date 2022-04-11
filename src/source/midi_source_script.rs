use crate::{FeedbackValue, RawMidiEvents};

pub trait MidiSourceScript {
    /// Returns raw MIDI bytes.
    fn execute(&self, input_value: FeedbackValue) -> Result<RawMidiEvents, &'static str>;
}
