use crate::{AbsoluteValue, RawMidiEvent};

pub trait MidiSourceScript {
    /// Returns raw MIDI bytes.
    fn execute(&self, input_value: AbsoluteValue) -> Result<Box<RawMidiEvent>, &'static str>;
}
