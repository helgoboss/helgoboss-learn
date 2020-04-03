use helgoboss_midi::{Midi14BitControlChangeMessage, MidiMessage, MidiParameterNumberMessage};

/// Incoming value which might be used to control something
pub enum MidiSourceValue<M: MidiMessage> {
    PlainMessage(M),
    ParameterNumberMessage(MidiParameterNumberMessage),
    FourteenBitControlChangeMessage(Midi14BitControlChangeMessage),
    TempoMessage { bpm: f64 },
}
