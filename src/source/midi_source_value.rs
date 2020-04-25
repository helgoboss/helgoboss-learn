use helgoboss_midi::{ControlChange14BitMessage, ParameterNumberMessage, ShortMessage};

/// Incoming value which might be used to control something
#[derive(Debug, Clone, PartialEq)]
pub enum MidiSourceValue<M: ShortMessage> {
    Plain(M),
    ParameterNumber(ParameterNumberMessage),
    ControlChange14Bit(ControlChange14BitMessage),
    Tempo { bpm: f64 },
}
