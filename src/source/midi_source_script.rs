use crate::{RawMidiEvent, UnitValue};

pub trait MidiSourceScript {
    /// Returns raw MIDI bytes.
    fn execute(&self, input_value: UnitValue) -> Result<Box<RawMidiEvent>, &'static str>;
}
