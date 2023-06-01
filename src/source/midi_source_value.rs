use crate::{DisplaySpecAddress, MidiSourceAddress, PatternByte, UnitValue};
use derive_more::Display;
use helgoboss_midi::{
    Channel, ControlChange14BitMessage, DataEntryByteOrder, ParameterNumberMessage, ShortMessage,
    ShortMessageFactory, StructuredShortMessage,
};
use std::convert::TryFrom;
use std::ops::RangeInclusive;

pub type RawMidiEvents = Vec<RawMidiEvent>;

/// Values produced when asking for feedback from MIDI sources.
///
/// At the moment, we always produce a final value and maybe a non-final one in addition, so this
/// isn't an enum.
#[derive(Clone, PartialEq, Debug)]
pub struct PreliminaryMidiSourceFeedbackValue<'a, M: ShortMessage> {
    /// A concrete MIDI message.
    pub final_value: MidiSourceValue<'a, M>,
    /// Request to set the color of one particular XTouch channel display.
    ///
    /// The XTouch doesn't provide a way to set the color for one particular channel, only one to
    /// set the colors of all channels at once. That means we need to keep the current color of
    /// each channel around as state, "integrate" these requests after collecting them from the
    /// sources and then build the final sys-ex message.
    pub x_touch_mackie_lcd_color_request: Option<XTouchMackieLcdColorRequest>,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct XTouchMackieLcdColorRequest {
    pub extender_index: u8,
    pub channel: Option<u8>,
    pub color_index: Option<u8>,
}

/// Incoming or outgoing value which might be used to control something or send feedback.
#[derive(Clone, PartialEq, Debug)]
pub enum MidiSourceValue<'a, M: ShortMessage> {
    // Feedback and control
    Plain(M),
    ParameterNumber(ParameterNumberMessage),
    ControlChange14Bit(ControlChange14BitMessage),
    /// We must take care not to allocate this in real-time thread!
    Raw {
        feedback_address_info: Option<RawFeedbackAddressInfo>,
        events: RawMidiEvents,
    },
    // Control-only
    Tempo(Bpm),
    // Control-only
    BorrowedSysEx(&'a [u8]),
}

/// For being able to reconstructing the source address for feedback purposes (in particular,
/// source takeover).
///
/// Also important for preventing duplicate feedback.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum RawFeedbackAddressInfo {
    Raw {
        variable_range: Option<RangeInclusive<usize>>,
    },
    Display {
        spec: DisplaySpecAddress,
    },
    Custom(MidiSourceAddress),
}

impl<'a, M: ShortMessage> MidiSourceValue<'a, M> {
    pub fn single_raw(
        feedback_address_info: Option<RawFeedbackAddressInfo>,
        event: RawMidiEvent,
    ) -> Self {
        Self::Raw {
            feedback_address_info,
            events: create_raw_midi_events_singleton(event),
        }
    }
}

pub fn create_raw_midi_events_singleton(event: RawMidiEvent) -> RawMidiEvents {
    vec![event]
}

impl<'a, M: ShortMessage + ShortMessageFactory + Copy> MidiSourceValue<'a, M> {
    pub fn extract_feedback_address(&self) -> Option<MidiSourceAddress> {
        use MidiSourceValue::*;
        let res = match self {
            Plain(m) => {
                use StructuredShortMessage::*;
                match m.to_structured() {
                    NoteOn {
                        channel,
                        key_number,
                        ..
                    }
                    | NoteOff {
                        channel,
                        key_number,
                        ..
                    } => MidiSourceAddress::Note {
                        channel,
                        key_number,
                    },
                    PolyphonicKeyPressure {
                        channel,
                        key_number,
                        ..
                    } => MidiSourceAddress::PolyphonicKeyPressure {
                        channel,
                        key_number,
                    },
                    ControlChange {
                        channel,
                        controller_number,
                        ..
                    } => MidiSourceAddress::ControlChange {
                        channel,
                        controller_number,
                        is_14_bit: false,
                    },
                    ProgramChange { channel, .. } => MidiSourceAddress::ProgramChange { channel },
                    ChannelPressure { channel, .. } => {
                        MidiSourceAddress::ChannelPressure { channel }
                    }
                    PitchBendChange { channel, .. } => {
                        MidiSourceAddress::PitchBendChange { channel }
                    }
                    // No feedback supported for other types of MIDI messages
                    _ => return None,
                }
            }
            ParameterNumber(msg) => MidiSourceAddress::ParameterNumber {
                channel: msg.channel(),
                number: msg.number(),
                is_registered: msg.is_registered(),
            },
            ControlChange14Bit(msg) => MidiSourceAddress::ControlChange {
                channel: msg.channel(),
                controller_number: msg.msb_controller_number(),
                is_14_bit: true,
            },
            Raw {
                feedback_address_info,
                events,
            } => match feedback_address_info.as_ref()? {
                RawFeedbackAddressInfo::Raw { variable_range } => MidiSourceAddress::Raw {
                    pattern: events
                        .first()?
                        .bytes()
                        .iter()
                        .enumerate()
                        .map(|(i, b)| {
                            if let Some(vr) = variable_range {
                                if vr.contains(&i) {
                                    PatternByte::Variable
                                } else {
                                    PatternByte::Fixed(*b)
                                }
                            } else {
                                PatternByte::Fixed(*b)
                            }
                        })
                        .collect(),
                },
                RawFeedbackAddressInfo::Display { spec } => {
                    MidiSourceAddress::Display { spec: spec.clone() }
                }
                RawFeedbackAddressInfo::Custom(addr) => addr.clone(),
            },
            // No feedback
            Tempo(_) | BorrowedSysEx(_) => return None,
        };
        Some(res)
    }

    pub fn channel(&self) -> Option<Channel> {
        use MidiSourceValue::*;
        match self {
            Plain(m) => m.channel(),
            ParameterNumber(m) => Some(m.channel()),
            ControlChange14Bit(m) => Some(m.channel()),
            _ => None,
        }
    }

    /// Might allocate!
    ///
    /// Not usable for producing feedback output that should participate in feedback relay
    /// (since BorrowedSysEx doesn't contain a feedback address).
    pub fn try_into_owned(self) -> Result<MidiSourceValue<'static, M>, &'static str> {
        use MidiSourceValue::*;
        let res = match self {
            Plain(v) => Plain(v),
            ParameterNumber(v) => ParameterNumber(v),
            ControlChange14Bit(v) => ControlChange14Bit(v),
            Tempo(v) => Tempo(v),
            Raw {
                feedback_address_info,
                events,
            } => Raw {
                feedback_address_info,
                events,
            },
            BorrowedSysEx(bytes) => {
                // Situations where we convert a borrowed message into an owned are not
                // situations in which we want to send a feedback value. So it's not bad that
                // we can't provide a feedback address here.
                let feedback_address_info = None;
                let event = RawMidiEvent::try_from_slice(0, bytes)?;
                MidiSourceValue::single_raw(feedback_address_info, event)
            }
        };
        Ok(res)
    }

    pub fn into_garbage(self) -> Option<RawMidiEvents> {
        use MidiSourceValue::*;
        match self {
            Raw { events, .. } => Some(events),
            _ => None,
        }
    }

    /// For values that are best sent raw, e.g. sys-ex.
    pub fn to_raw(&self) -> Option<impl Iterator<Item = &RawMidiEvent>> {
        use MidiSourceValue::*;
        match self {
            Raw { events, .. } => Some(events.iter()),
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
            Tempo(_) | Raw { .. } | BorrowedSysEx(_) => [None; 4],
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

impl Default for RawMidiEvent {
    fn default() -> Self {
        Self {
            frame_offset: 0,
            size: 0,
            midi_message: [0; RawMidiEvent::MAX_LENGTH],
        }
    }
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
