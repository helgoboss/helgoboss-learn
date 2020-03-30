use helgoboss_midi::{Midi14BitCcMessage, MidiMessage, MidiParameterNumberMessage};

/// Incoming value which might be used to control something
pub enum MidiSourceValue<M: MidiMessage> {
    PlainMessage(M),
    ParameterNumberMessage(MidiParameterNumberMessage),
    FourteenBitCcMessage(Midi14BitCcMessage),
    TempoMessage { bpm: f64 },
}
