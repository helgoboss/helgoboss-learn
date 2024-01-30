use crate::{FeedbackValue, MidiSourceAddress, RawMidiEvents};
use std::borrow::Cow;

// The lifetime 'a is necessary in case we want to parameterize the lifetime
// of the additional input dynamically. An alternative would have been to
// require the additional input type to be static and take it by reference.
// But that would be less generic.
pub trait MidiSourceScript<'a> {
    type AdditionalInput: Default;

    /// Returns raw MIDI bytes.
    fn execute(
        &self,
        input_value: FeedbackValue,
        additional_input: Self::AdditionalInput,
    ) -> Result<MidiSourceScriptOutcome, Cow<'static, str>>;
}

pub struct MidiSourceScriptOutcome {
    pub address: Option<MidiSourceAddress>,
    pub events: RawMidiEvents,
}
