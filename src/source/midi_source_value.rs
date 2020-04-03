use helgoboss_midi::{MidiControlChange14BitMessage, MidiMessage, MidiParameterNumberMessage};

/// Incoming value which might be used to control something
#[derive(Debug, Clone, PartialEq)]
pub enum MidiSourceValue<M: MidiMessage> {
    Plain(M),
    ParameterNumber(MidiParameterNumberMessage),
    ControlChange14Bit(MidiControlChange14BitMessage),
    Tempo { bpm: f64 },
}
