use crate::UnitValue;
use derive_more::Display;
use helgoboss_midi::{
    ControlChange14BitMessage, DataEntryByteOrder, ParameterNumberMessage, ShortMessage,
    ShortMessageFactory,
};
use std::convert::TryFrom;

/// Incoming value which might be used to control something
#[derive(Clone, PartialEq, Debug)]
pub enum MidiSourceValue<'a, M: ShortMessage> {
    Plain(M),
    ParameterNumber(ParameterNumberMessage),
    ControlChange14Bit(ControlChange14BitMessage),
    Tempo(Bpm),
    /// We must take care not to allocate this in real-time thread!
    Raw(Vec<RawMidiEvent>),
    BorrowedSysEx(&'a [u8]),
    // TODO-medium Not used so far. Just to show that we could defer raw-message generation to the
    //  place at which we need to send it. We adjusted the signature so that any of these types
    //  could potentially generate a bunch of raw MIDI messages - without allocation, using
    //  iterators, one raw message at a time.
    DisplaySpecific(()),
}

impl<'a, M: ShortMessage + ShortMessageFactory + Copy> MidiSourceValue<'a, M> {
    /// Might allocate!
    pub fn try_to_owned(self) -> Result<MidiSourceValue<'static, M>, &'static str> {
        use MidiSourceValue::*;
        let res = match self {
            Plain(v) => Plain(v),
            ParameterNumber(v) => ParameterNumber(v),
            ControlChange14Bit(v) => ControlChange14Bit(v),
            Tempo(v) => Tempo(v),
            Raw(v) => Raw(v),
            DisplaySpecific(v) => DisplaySpecific(v),
            BorrowedSysEx(bytes) => Raw(vec![RawMidiEvent::try_from_slice(0, bytes)?]),
        };
        Ok(res)
    }

    pub fn into_garbage(self) -> Option<Vec<RawMidiEvent>> {
        use MidiSourceValue::*;
        match self {
            Raw(events) => Some(events),
            _ => None,
        }
    }

    /// For values that are best sent raw, e.g. sys-ex.
    pub fn to_raw(&self) -> Option<impl Iterator<Item = &RawMidiEvent>> {
        use MidiSourceValue::*;
        match self {
            Raw(events) => Some(events.iter()),
            _ => None,
        }
    }

    /// For values that are best sent as short messages.
    pub fn to_short_messages(
        &self,
        nrpn_data_entry_byte_order: DataEntryByteOrder,
    ) -> [Option<M>; 4] {
        use MidiSourceValue::*;
        match self {
            Plain(msg) => [Some(*msg), None, None, None],
            ParameterNumber(msg) => msg.to_short_messages(nrpn_data_entry_byte_order),
            ControlChange14Bit(msg) => {
                let inner_shorts = msg.to_short_messages();
                [Some(inner_shorts[0]), Some(inner_shorts[1]), None, None]
            }
            Tempo(_) | Raw(_) | BorrowedSysEx(_) | DisplaySpecific(_) => [None; 4],
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
///
/// Conforms to the LongMidiEvent in `reaper-medium` but the goal of `helgoboss-learn` is to be
/// DAW-agnostic, so we have to recreate the lowest common denominator.
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

    /// If you already have a slice, use this. If you are just building something, `try_from_iter`
    /// is probably more efficient.
    pub fn try_from_slice(frame_offset: u32, midi_message: &[u8]) -> Result<Self, &'static str> {
        if midi_message.len() > Self::MAX_LENGTH {
            return Err("given MIDI message too long");
        }
        let mut array = [0; Self::MAX_LENGTH];
        // TODO-low I think copying from a slice is the only way to go, even we own a vec or array.
        //  REAPER's struct layout requires us to put something in front of the vec, which is
        //  not or at least not easily possible without copying.
        array[..midi_message.len()].copy_from_slice(midi_message);
        Ok(Self::new(frame_offset, midi_message.len() as _, array))
    }

    pub fn try_from_iter<T: IntoIterator<Item = u8>>(
        frame_offset: u32,
        iter: T,
    ) -> Result<Self, &'static str> {
        let mut array = [0; Self::MAX_LENGTH];
        let mut i = 0usize;
        for b in iter {
            if i == Self::MAX_LENGTH {
                return Err("given content too long");
            }
            let elem = unsafe { array.get_unchecked_mut(i) };
            *elem = b;
            i += 1;
        }
        Ok(Self::new(frame_offset, i as u32, array))
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
