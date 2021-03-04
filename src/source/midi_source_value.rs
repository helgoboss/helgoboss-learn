use crate::UnitValue;
use derive_more::Display;
use helgoboss_midi::{
    ControlChange14BitMessage, DataEntryByteOrder, ParameterNumberMessage, ShortMessage,
    ShortMessageFactory,
};
use std::convert::TryFrom;

/// Incoming value which might be used to control something
#[derive(Clone, PartialEq, Debug)]
pub enum MidiSourceValue<M: ShortMessage> {
    Plain(M),
    ParameterNumber(ParameterNumberMessage),
    ControlChange14Bit(ControlChange14BitMessage),
    Tempo(Bpm),
    /// We must take care not to allocate this in real-time thread! At the moment only feedback
    /// is supported with this source but once we support control, this gets relevant.
    SystemExclusive(Box<RawMidiEvent>),
}

impl<M: ShortMessage + ShortMessageFactory + Copy> MidiSourceValue<M> {
    pub fn to_short_messages(
        &self,
        nrpn_data_entry_byte_order: DataEntryByteOrder,
    ) -> [Option<M>; 4] {
        match self {
            MidiSourceValue::Plain(msg) => [Some(*msg), None, None, None],
            MidiSourceValue::ParameterNumber(msg) => {
                msg.to_short_messages(nrpn_data_entry_byte_order)
            }
            MidiSourceValue::ControlChange14Bit(msg) => {
                let inner_shorts = msg.to_short_messages();
                [Some(inner_shorts[0]), Some(inner_shorts[1]), None, None]
            }
            MidiSourceValue::Tempo(_) | MidiSourceValue::SystemExclusive(_) => [None; 4],
        }
    }
}

/// This represents a tempo measured in beats per minute.
#[derive(Copy, Clone, PartialEq, PartialOrd, Debug, Default, Display)]
pub struct Bpm(pub(crate) f64);

impl Bpm {
    /// The minimum possible value (1.0 bpm).
    pub const MIN: Bpm = Bpm(1.0);

    /// The maximum possible value (960.0 bpm).
    pub const MAX: Bpm = Bpm(960.0);

    /// Checks if the given value is within the BPM range supported by REAPER.
    pub fn is_valid(value: f64) -> bool {
        Bpm::MIN.get() <= value && value <= Bpm::MAX.get()
    }

    /// Creates a BPM value.
    ///
    /// # Panics
    ///
    /// This function panics if the given value is not within the BPM range supported by REAPER
    /// `(1.0..=960.0)`.
    pub fn new(value: f64) -> Bpm {
        assert!(Bpm::is_valid(value));
        Bpm(value)
    }

    /// Creates a BPM value from the given normalized value.
    pub fn from_unit_value(value: UnitValue) -> Bpm {
        let min = Bpm::MIN.get();
        let span = Bpm::MAX.get() - min;
        Bpm(min + value.get() * span)
    }

    /// Creates a BPM value from the given normalized value.
    pub fn to_unit_value(self) -> UnitValue {
        let min = Bpm::MIN.get();
        let span = Bpm::MAX.get() - min;
        UnitValue::new((self.get() - min) / span)
    }

    /// Returns the wrapped value.
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl std::str::FromStr for Bpm {
    type Err = &'static str;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let primitive = f64::from_str(source).map_err(|_| "not a valid decimal number")?;
        if !Bpm::is_valid(primitive) {
            return Err("not in the allowed BPM range");
        }
        Ok(Bpm(primitive))
    }
}

impl TryFrom<f64> for Bpm {
    type Error = &'static str;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if !Self::is_valid(value) {
            return Err("value must be between 1.0 and 960.0");
        }
        Ok(Bpm(value))
    }
}

/// Raw MIDI data which is compatible to both VST and REAPER MIDI data structures. The REAPER
/// struct is more picky in that it needs offset and size directly in front of the raw data whereas
/// the VST struct allows the data to be at a different address. That's why we need to follow the
/// REAPER requirement.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(C)]
pub struct RawMidiEvent {
    /// A MIDI frame offset.
    ///
    /// This is a 1/1024000 of a second, *not* a sample frame!
    frame_offset: i32,
    size: i32,
    midi_message: [u8; RawMidiEvent::MAX_LENGTH],
}

impl RawMidiEvent {
    pub const MAX_LENGTH: usize = 256;

    pub fn new(frame_offset: u32, size: u32, midi_message: [u8; Self::MAX_LENGTH]) -> Self {
        Self {
            frame_offset: frame_offset as _,
            size: size as _,
            midi_message,
        }
    }

    pub fn try_from_slice(frame_offset: u32, midi_message: &[u8]) -> Result<Self, &'static str> {
        if midi_message.len() > Self::MAX_LENGTH {
            return Err("given MIDI message too long");
        }
        let mut array = [0; Self::MAX_LENGTH];
        // TODO-low I think copying from a slice is the only way to go, even we own a vec or array.
        //  REAPER's struct layout requires us to put something in front of the vec, which is
        //  not or at least not easily possible without copying.
        array[..midi_message.len()].copy_from_slice(&midi_message);
        Ok(Self::new(frame_offset, midi_message.len() as _, array))
    }

    pub fn bytes(&self) -> &[u8] {
        &self.midi_message[..self.size as usize]
    }
}

#[cfg(feature = "reaper-low")]
impl AsRef<reaper_low::raw::MIDI_event_t> for RawMidiEvent {
    fn as_ref(&self) -> &reaper_low::raw::MIDI_event_t {
        unsafe { &*(self as *const RawMidiEvent as *const reaper_low::raw::MIDI_event_t) }
    }
}
