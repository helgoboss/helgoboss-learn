use crate::{AbsoluteValue, RawMidiEvents};

pub trait MidiSourceScript {
    /// Returns raw MIDI bytes.
    fn execute(&self, input_value: AbsoluteValue) -> Result<RawMidiEvents, &'static str>;
}
