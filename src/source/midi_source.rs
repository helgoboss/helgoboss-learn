use crate::{
    format_percentage_without_unit, parse_percentage_without_unit, AbsoluteValue, Bpm,
    ControlValue, DetailedSourceCharacter, DiscreteIncrement, FeedbackValue, Fraction,
    MidiSourceScript, MidiSourceValue, RawFeedbackAddressInfo, RawMidiEvent, RawMidiPattern,
    UnitValue,
};
use core::iter;
use derivative::Derivative;
use derive_more::Display;
use enum_iterator::IntoEnumIterator;
use num_enum::{IntoPrimitive, TryFromPrimitive};

use helgoboss_midi::{
    Channel, ControlChange14BitMessage, ControllerNumber, DataType, KeyNumber,
    ParameterNumberMessage, RawShortMessage, ShortMessage, ShortMessageFactory, ShortMessageType,
    StructuredShortMessage, U14, U7,
};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "serde_repr")]
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::convert::{TryFrom, TryInto};
use std::ops::Range;

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    IntoEnumIterator,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
)]
#[cfg_attr(feature = "serde_repr", derive(Serialize_repr, Deserialize_repr))]
#[repr(usize)]
pub enum SourceCharacter {
    #[display(fmt = "Range element (knob, fader, etc.)")]
    RangeElement = 0,
    #[display(fmt = "Button (momentary)")]
    MomentaryButton = 1,
    #[display(fmt = "Encoder (relative type 1)")]
    Encoder1 = 2,
    #[display(fmt = "Encoder (relative type 2)")]
    Encoder2 = 3,
    #[display(fmt = "Encoder (relative type 3)")]
    Encoder3 = 4,
    /// This exists as a workaround for buttons that only toggle and can't be configured to work
    /// as momentary buttons. A source with this character will always emit 1, even if the
    /// hardware toggle is switching to off.   
    #[display(fmt = "Toggle-only button (avoid!)")]
    ToggleButton = 5,
}

impl Default for SourceCharacter {
    fn default() -> Self {
        SourceCharacter::RangeElement
    }
}

impl SourceCharacter {
    /// Returns whether sources with this character emit relative increments instead of absolute
    /// values.
    pub fn emits_increments(&self) -> bool {
        use SourceCharacter::*;
        matches!(self, Encoder1 | Encoder2 | Encoder3)
    }

    pub fn possible_detailed_characters(&self) -> Vec<DetailedSourceCharacter> {
        use SourceCharacter::*;
        match self {
            RangeElement => vec![DetailedSourceCharacter::RangeControl],
            MomentaryButton => vec![
                DetailedSourceCharacter::MomentaryOnOffButton,
                DetailedSourceCharacter::MomentaryVelocitySensitiveButton,
            ],
            Encoder1 | Encoder2 | Encoder3 => vec![DetailedSourceCharacter::Relative],
            ToggleButton => vec![DetailedSourceCharacter::PressOnlyButton],
        }
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    IntoEnumIterator,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
)]
#[cfg_attr(feature = "serde_repr", derive(Serialize_repr, Deserialize_repr))]
#[repr(usize)]
pub enum MidiClockTransportMessage {
    Start = 0,
    Continue = 1,
    Stop = 2,
}

impl Default for MidiClockTransportMessage {
    fn default() -> Self {
        MidiClockTransportMessage::Start
    }
}

impl From<MidiClockTransportMessage> for ShortMessageType {
    fn from(msg: MidiClockTransportMessage) -> Self {
        use MidiClockTransportMessage::*;
        match msg {
            Start => ShortMessageType::Start,
            Continue => ShortMessageType::Continue,
            Stop => ShortMessageType::Stop,
        }
    }
}

#[derive(Clone, Debug, Derivative)]
#[derivative(PartialEq)]
pub enum MidiSource<S: MidiSourceScript> {
    NoteVelocity {
        channel: Option<Channel>,
        key_number: Option<KeyNumber>,
    },
    NoteKeyNumber {
        channel: Option<Channel>,
    },
    // ShortMessageType::PolyphonicKeyPressure
    PolyphonicKeyPressureAmount {
        channel: Option<Channel>,
        key_number: Option<KeyNumber>,
    },
    // ShortMessageType::ControlChange
    ControlChangeValue {
        channel: Option<Channel>,
        controller_number: Option<ControllerNumber>,
        custom_character: SourceCharacter,
    },
    // ShortMessageType::ProgramChange
    ProgramChangeNumber {
        channel: Option<Channel>,
    },
    // ShortMessageType::ChannelPressure
    ChannelPressureAmount {
        channel: Option<Channel>,
    },
    // ShortMessageType::PitchBendChange
    PitchBendChangeValue {
        channel: Option<Channel>,
    },
    // ControlChange14BitMessage
    ControlChange14BitValue {
        channel: Option<Channel>,
        msb_controller_number: Option<ControllerNumber>,
        custom_character: SourceCharacter,
    },
    // ParameterNumberMessage
    ParameterNumberValue {
        channel: Option<Channel>,
        number: Option<U14>,
        is_14_bit: Option<bool>,
        is_registered: Option<bool>,
        custom_character: SourceCharacter,
    },
    // ShortMessageType::TimingClock
    ClockTempo,
    // ShortMessageType::{Start, Continue, Stop}
    ClockTransport {
        message: MidiClockTransportMessage,
    },
    // E.g. SysEx
    Raw {
        pattern: RawMidiPattern,
        custom_character: SourceCharacter,
    },
    // For advanced programmable feedback (e.g. to drive hardware displays).
    Script {
        #[derivative(PartialEq = "ignore")]
        script: Option<S>,
    },
    Display {
        spec: DisplaySpec,
    },
}

/// Uniquely addresses a source (e.g. used for source takeover and filtering).
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub enum MidiSourceAddress {
    Note {
        channel: Channel,
        key_number: KeyNumber,
    },
    PolyphonicKeyPressure {
        channel: Channel,
        key_number: KeyNumber,
    },
    ControlChange {
        channel: Channel,
        controller_number: ControllerNumber,
        is_14_bit: bool,
    },
    ProgramChange {
        channel: Channel,
    },
    ChannelPressure {
        channel: Channel,
    },
    PitchBendChange {
        channel: Channel,
    },
    ParameterNumber {
        channel: Channel,
        number: U14,
        is_registered: bool,
    },
    Display {
        spec: DisplaySpec,
    },
    Raw {
        pattern: Vec<PatternByte>,
    },
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum PatternByte {
    Fixed(u8),
    Variable,
}

impl<S: MidiSourceScript> MidiSource<S> {
    /// This will be very fast except maybe for raw sources.
    pub fn extract_feedback_address(&self) -> Option<MidiSourceAddress> {
        use MidiSource::*;
        let res = match self {
            NoteVelocity {
                channel: Some(ch),
                key_number: Some(kn),
            } => MidiSourceAddress::Note {
                channel: *ch,
                key_number: *kn,
            },
            PolyphonicKeyPressureAmount {
                channel: Some(ch),
                key_number: Some(kn),
            } => MidiSourceAddress::PolyphonicKeyPressure {
                channel: *ch,
                key_number: *kn,
            },
            ControlChangeValue {
                channel: Some(ch),
                controller_number: Some(cn),
                ..
            } => MidiSourceAddress::ControlChange {
                channel: *ch,
                controller_number: *cn,
                is_14_bit: false,
            },
            ProgramChangeNumber { channel: Some(ch) } => {
                MidiSourceAddress::ProgramChange { channel: *ch }
            }
            ChannelPressureAmount { channel: Some(ch) } => {
                MidiSourceAddress::ChannelPressure { channel: *ch }
            }
            PitchBendChangeValue { channel: Some(ch) } => {
                MidiSourceAddress::PitchBendChange { channel: *ch }
            }
            ControlChange14BitValue {
                channel: Some(ch),
                msb_controller_number: Some(cn),
                ..
            } => MidiSourceAddress::ControlChange {
                channel: *ch,
                controller_number: *cn,
                is_14_bit: true,
            },
            ParameterNumberValue {
                channel: Some(ch),
                number: Some(n),
                is_registered: Some(is_registered),
                ..
            } => MidiSourceAddress::ParameterNumber {
                channel: *ch,
                number: *n,
                is_registered: *is_registered,
            },
            Display { spec } => MidiSourceAddress::Display { spec: *spec },
            Raw { pattern, .. } => MidiSourceAddress::Raw {
                pattern: pattern.to_pattern_bytes(),
            },
            // No static analysis possible
            Script { .. } => return None,
            // No feedback
            ClockTempo | ClockTransport { .. } | NoteKeyNumber { .. } => return None,
            // Non-feedback-compatible configurations (e.g. channel == <Any>)
            _ => return None,
        };
        Some(res)
    }

    /// Checks if the given message is directed to the same address as the one of this source.
    ///
    /// Used for:
    ///
    /// -  Source takeover (feedback)
    pub fn has_same_feedback_address_as_value(
        &self,
        value: &MidiSourceValue<RawShortMessage>,
    ) -> bool {
        if let (Some(addr1), Some(addr2)) = (
            self.extract_feedback_address(),
            value.extract_feedback_address(),
        ) {
            addr1 == addr2
        } else {
            false
        }
    }

    /// Checks if this and the given source share the same address.
    ///
    /// For performance reasons, this doesn't try to match sources of different types that could
    /// end up the same on byte level. E.g. right now when doing source filtering, an incoming pitch
    /// bend message wouldn't match a Raw source that simulates a pitch bend message and therefore
    /// the mapping with the Raw source would not show up.
    ///
    /// If we need that one day, it should be not difficult to implement. However, I think it's not
    /// urgent at all. Just because one *can* simulate short MIDI messages with the Raw source
    /// doesn't mean it's encouraged.
    ///
    /// Used for:
    ///
    /// - Feedback diffing
    pub fn has_same_feedback_address_as_source(&self, other: &Self) -> bool {
        self.extract_feedback_address() == other.extract_feedback_address()
    }

    /// Used for creating sources when learning.
    ///
    /// Might allocate!
    pub fn from_source_value(
        source_value: MidiSourceValue<impl ShortMessage>,
        custom_character_hint: Option<SourceCharacter>,
    ) -> Option<Self> {
        use MidiSourceValue::*;
        let source = match source_value {
            ParameterNumber(msg) => MidiSource::ParameterNumberValue {
                channel: Some(msg.channel()),
                number: Some(msg.number()),
                is_14_bit: Some(msg.is_14_bit()),
                is_registered: Some(msg.is_registered()),
                custom_character: custom_character_hint.unwrap_or_default(),
            },
            ControlChange14Bit(msg) => MidiSource::ControlChange14BitValue {
                channel: Some(msg.channel()),
                msb_controller_number: Some(msg.msb_controller_number()),
                custom_character: custom_character_hint.unwrap_or_default(),
            },
            Tempo(_) => MidiSource::ClockTempo,
            Plain(msg) => MidiSource::from_short_message(msg, custom_character_hint)?,
            BorrowedSysEx(msg) => MidiSource::from_raw(msg),
            // Important (and working) for learning.
            Raw { events, .. } => MidiSource::from_raw(events.first()?.bytes()),
        };
        Some(source)
    }

    /// Allocates!
    pub fn from_raw(msg: &[u8]) -> Self {
        MidiSource::Raw {
            pattern: RawMidiPattern::fixed_from_slice(msg),
            custom_character: Default::default(),
        }
    }

    fn from_short_message(
        msg: impl ShortMessage,
        custom_character_hint: Option<SourceCharacter>,
    ) -> Option<Self> {
        use StructuredShortMessage::*;
        let source = match msg.to_structured() {
            NoteOn {
                channel,
                key_number,
                ..
            }
            | NoteOff {
                channel,
                key_number,
                ..
            } => MidiSource::NoteVelocity {
                channel: Some(channel),
                key_number: Some(key_number),
            },
            PolyphonicKeyPressure {
                channel,
                key_number,
                ..
            } => MidiSource::PolyphonicKeyPressureAmount {
                channel: Some(channel),
                key_number: Some(key_number),
            },
            ControlChange {
                channel,
                controller_number,
                ..
            } => MidiSource::ControlChangeValue {
                channel: Some(channel),
                controller_number: Some(controller_number),
                custom_character: custom_character_hint.unwrap_or_default(),
            },
            ProgramChange { channel, .. } => MidiSource::ProgramChangeNumber {
                channel: Some(channel),
            },
            ChannelPressure { channel, .. } => MidiSource::ChannelPressureAmount {
                channel: Some(channel),
            },
            PitchBendChange { channel, .. } => MidiSource::PitchBendChangeValue {
                channel: Some(channel),
            },
            TimingClock => MidiSource::ClockTempo,
            Start => MidiSource::ClockTransport {
                message: MidiClockTransportMessage::Start,
            },
            Continue => MidiSource::ClockTransport {
                message: MidiClockTransportMessage::Continue,
            },
            Stop => MidiSource::ClockTransport {
                message: MidiClockTransportMessage::Stop,
            },
            _ => {
                return None;
            }
        };
        Some(source)
    }

    pub fn channel(&self) -> Option<Channel> {
        use MidiSource::*;
        match self {
            NoteVelocity { channel, .. }
            | NoteKeyNumber { channel }
            | PolyphonicKeyPressureAmount { channel, .. }
            | ControlChangeValue { channel, .. }
            | ProgramChangeNumber { channel }
            | ChannelPressureAmount { channel }
            | PitchBendChangeValue { channel }
            | ControlChange14BitValue { channel, .. }
            | ParameterNumberValue { channel, .. } => *channel,
            ClockTempo | ClockTransport { .. } | Raw { .. } | Script { .. } | Display { .. } => {
                None
            }
        }
    }

    pub fn character(&self) -> SourceCharacter {
        use MidiSource::*;
        match self {
            NoteVelocity { .. } => SourceCharacter::MomentaryButton,
            // TODO-low Introduce new character "Trigger"
            ClockTransport { .. } => SourceCharacter::MomentaryButton,
            Raw {
                custom_character, ..
            }
            | ControlChangeValue {
                custom_character, ..
            }
            | ControlChange14BitValue {
                custom_character, ..
            }
            | ParameterNumberValue {
                custom_character, ..
            } => *custom_character,
            NoteKeyNumber { .. }
            | PolyphonicKeyPressureAmount { .. }
            | ProgramChangeNumber { .. }
            | ChannelPressureAmount { .. }
            | PitchBendChangeValue { .. }
            | Script { .. }
            | Display { .. }
            | ClockTempo => SourceCharacter::RangeElement,
        }
    }

    pub fn possible_detailed_characters(&self) -> Vec<DetailedSourceCharacter> {
        use MidiSource::*;
        match self {
            NoteVelocity { .. } => vec![
                DetailedSourceCharacter::MomentaryVelocitySensitiveButton,
                DetailedSourceCharacter::MomentaryOnOffButton,
            ],
            ClockTransport { .. } => vec![DetailedSourceCharacter::PressOnlyButton],
            // User can choose.
            Raw {
                custom_character, ..
            }
            | ControlChangeValue {
                custom_character, ..
            }
            | ControlChange14BitValue {
                custom_character, ..
            } => custom_character.possible_detailed_characters(),
            ParameterNumberValue {
                custom_character,
                is_14_bit,
                ..
            } => {
                if *is_14_bit == Some(true) {
                    custom_character.possible_detailed_characters()
                } else {
                    let mut res = custom_character.possible_detailed_characters();
                    res.push(DetailedSourceCharacter::Relative);
                    res
                }
            }
            // Usually a range control but sometimes more like a button (e.g. see #316).
            ProgramChangeNumber { .. } | ChannelPressureAmount { .. } => vec![
                DetailedSourceCharacter::RangeControl,
                DetailedSourceCharacter::MomentaryOnOffButton,
                DetailedSourceCharacter::PressOnlyButton,
            ],
            // Usually a range control but could also be a velocity-sensitive button.
            PolyphonicKeyPressureAmount { .. } | PitchBendChangeValue { .. } => {
                vec![
                    DetailedSourceCharacter::RangeControl,
                    DetailedSourceCharacter::MomentaryVelocitySensitiveButton,
                    DetailedSourceCharacter::MomentaryOnOffButton,
                    DetailedSourceCharacter::PressOnlyButton,
                ]
            }
            // We exposed this as range-only ("key range") before but this actually also works as
            // buttons that are never released.
            NoteKeyNumber { .. } => {
                vec![
                    DetailedSourceCharacter::RangeControl,
                    DetailedSourceCharacter::PressOnlyButton,
                ]
            }
            // Special targets for which we can safely say it's a range.
            ClockTempo => vec![DetailedSourceCharacter::RangeControl],
            // Feedback-only but characters also matter for feedback.
            Script { .. } => {
                vec![
                    DetailedSourceCharacter::RangeControl,
                    DetailedSourceCharacter::MomentaryVelocitySensitiveButton,
                    DetailedSourceCharacter::MomentaryOnOffButton,
                    DetailedSourceCharacter::PressOnlyButton,
                ]
            }
            // Feedback-only but characters also matter for feedback.
            Display { .. } => vec![DetailedSourceCharacter::RangeControl],
        }
    }

    /// Determines the appropriate control value from the given MIDI source value. If this source
    /// doesn't process values of that type, it returns None.
    pub fn control(&self, value: &MidiSourceValue<impl ShortMessage>) -> Option<ControlValue> {
        use MidiSource as S;
        use MidiSourceValue::*;
        use StructuredShortMessage::*;
        match self {
            S::NoteVelocity {
                channel,
                key_number,
            } => match value {
                Plain(msg) => match msg.to_structured() {
                    NoteOn {
                        channel: ch,
                        key_number: kn,
                        velocity,
                    } if matches(ch, *channel) && matches(kn, *key_number) => {
                        Some(abs(normalize_7_bit(velocity)))
                    }
                    NoteOff {
                        channel: ch,
                        key_number: kn,
                        ..
                    } if matches(ch, *channel) && matches(kn, *key_number) => {
                        Some(abs(MIN_U7_FRACTION))
                    }
                    _ => None,
                },
                _ => None,
            },
            S::NoteKeyNumber { channel } => match value {
                Plain(msg) => match msg.to_structured() {
                    NoteOn {
                        channel: ch,
                        key_number,
                        velocity,
                    } if velocity > U7::MIN && matches(ch, *channel) => {
                        Some(abs(normalize_7_bit(key_number)))
                    }
                    _ => None,
                },
                _ => None,
            },
            S::PitchBendChangeValue { channel } => match value {
                Plain(msg) => match msg.to_structured() {
                    PitchBendChange {
                        channel: ch,
                        pitch_bend_value,
                    } if matches(ch, *channel) => {
                        Some(abs(normalize_14_bit_centered(pitch_bend_value)))
                    }
                    _ => None,
                },
                _ => None,
            },
            S::ChannelPressureAmount { channel } => match value {
                Plain(msg) => match msg.to_structured() {
                    ChannelPressure {
                        channel: ch,
                        pressure_amount,
                    } if matches(ch, *channel) => Some(abs(normalize_7_bit(pressure_amount))),
                    _ => None,
                },
                _ => None,
            },
            S::ProgramChangeNumber { channel } => match value {
                Plain(msg) => match msg.to_structured() {
                    ProgramChange {
                        channel: ch,
                        program_number,
                    } if matches(ch, *channel) => Some(abs(normalize_7_bit(program_number))),
                    _ => None,
                },
                _ => None,
            },
            S::PolyphonicKeyPressureAmount {
                channel,
                key_number,
            } => match value {
                Plain(msg) => match msg.to_structured() {
                    PolyphonicKeyPressure {
                        channel: ch,
                        key_number: kn,
                        pressure_amount,
                    } if matches(ch, *channel) && matches(kn, *key_number) => {
                        Some(abs(normalize_7_bit(pressure_amount)))
                    }
                    _ => None,
                },
                _ => None,
            },
            S::ControlChangeValue {
                channel,
                controller_number,
                custom_character,
            } => match value {
                Plain(msg) => match msg.to_structured() {
                    ControlChange {
                        channel: ch,
                        controller_number: cn,
                        control_value,
                    } if matches(ch, *channel) && matches(cn, *controller_number) => {
                        calc_control_value_from_n_bit_cc(*custom_character, control_value, 7).ok()
                    }
                    _ => None,
                },
                _ => None,
            },
            S::ControlChange14BitValue {
                channel,
                msb_controller_number,
                custom_character,
            } => match value {
                ControlChange14Bit(msg)
                    if matches(msg.channel(), *channel)
                        && matches(msg.msb_controller_number(), *msb_controller_number) =>
                {
                    calc_control_value_from_n_bit_cc(*custom_character, msg.value(), 14).ok()
                }
                _ => None,
            },
            S::ParameterNumberValue {
                channel,
                number,
                is_14_bit,
                is_registered,
                custom_character,
            } => match value {
                ParameterNumber(msg)
                    if matches(msg.channel(), *channel)
                        && matches(msg.number(), *number)
                        && matches(msg.is_14_bit(), *is_14_bit)
                        && matches(msg.is_registered(), *is_registered) =>
                {
                    match msg.data_type() {
                        DataType::DataEntry => {
                            if msg.is_14_bit() {
                                calc_control_value_from_n_bit_cc(*custom_character, msg.value(), 14)
                                    .ok()
                            } else {
                                let u7_value = U7::try_from(msg.value()).unwrap();
                                calc_control_value_from_n_bit_cc(*custom_character, u7_value, 7)
                                    .ok()
                            }
                        }
                        DataType::DataIncrement => {
                            (msg.value().get() as i32).try_into().ok().map(rel)
                        }
                        DataType::DataDecrement => {
                            (-(msg.value().get() as i32)).try_into().ok().map(rel)
                        }
                    }
                }
                _ => None,
            },
            S::ClockTransport { message } => match value {
                Plain(msg) if msg.r#type() == (*message).into() => Some(abs(Fraction::new_max(1))),
                _ => None,
            },
            S::ClockTempo => match value {
                Tempo(bpm) => Some(ControlValue::AbsoluteContinuous(bpm.to_unit_value())),
                _ => None,
            },
            S::Raw {
                pattern,
                custom_character,
            } => {
                let bytes = match value {
                    Raw { events, .. } => events.first()?.bytes(),
                    BorrowedSysEx(bytes) => bytes,
                    _ => return None,
                };
                let fraction = pattern.match_and_capture(bytes)?;
                if fraction.max_val() == 0 {
                    // Fixed pattern with no variable parts. This should act like a trigger!
                    Some(ControlValue::AbsoluteContinuous(UnitValue::MAX))
                } else {
                    calc_control_value_from_n_bit_cc(
                        *custom_character,
                        fraction.actual(),
                        pattern.resolution() as _,
                    )
                    .ok()
                }
            }
            // Feedback-only forever.
            S::Script { .. } | S::Display { .. } => None,
        }
    }

    /// Checks if this source consumes the given MIDI message. This is for sources whose events are
    /// composed of multiple MIDI messages, which is 14-bit CC and (N)RPN.
    // TODO-low Don't take ShortMessage by reference, never!
    pub fn consumes(&self, msg: &impl ShortMessage) -> bool {
        use MidiSource::*;
        use StructuredShortMessage::*;
        match self {
            ControlChange14BitValue {
                channel,
                msb_controller_number,
                ..
            } => match msg.to_structured() {
                ControlChange {
                    channel: ch,
                    controller_number,
                    ..
                } => {
                    matches(ch, *channel)
                        && (matches(controller_number, *msb_controller_number)
                            || matches(
                                controller_number,
                                msb_controller_number.map(|n| {
                                    n.corresponding_14_bit_lsb_controller_number().unwrap()
                                }),
                            ))
                }
                _ => false,
            },
            ParameterNumberValue { channel, .. } => match msg.to_structured() {
                ControlChange {
                    channel: ch,
                    controller_number,
                    ..
                } => {
                    matches(ch, *channel)
                        && controller_number.is_parameter_number_message_controller_number()
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Returns an appropriate MIDI source value for the given feedback value if feedback is
    /// supported by this source.
    pub fn feedback<M: ShortMessage + ShortMessageFactory>(
        &self,
        feedback_value: FeedbackValue,
    ) -> Option<MidiSourceValue<'static, M>> {
        use MidiSource::*;
        use MidiSourceValue as V;
        match self {
            NoteVelocity {
                channel: Some(ch),
                key_number: Some(kn),
            } => Some(V::Plain(M::note_on(
                *ch,
                *kn,
                denormalize_7_bit(feedback_value.to_numeric()?),
            ))),
            NoteKeyNumber { channel: Some(ch) } => Some(V::Plain(M::note_on(
                *ch,
                denormalize_7_bit(feedback_value.to_numeric()?),
                U7::MAX,
            ))),
            PolyphonicKeyPressureAmount {
                channel: Some(ch),
                key_number: Some(kn),
            } => Some(V::Plain(M::polyphonic_key_pressure(
                *ch,
                *kn,
                denormalize_7_bit(feedback_value.to_numeric()?),
            ))),
            ControlChangeValue {
                channel: Some(ch),
                controller_number: Some(cn),
                ..
            } => Some(V::Plain(M::control_change(
                *ch,
                *cn,
                denormalize_7_bit(feedback_value.to_numeric()?),
            ))),
            ProgramChangeNumber { channel: Some(ch) } => Some(V::Plain(M::program_change(
                *ch,
                denormalize_7_bit(feedback_value.to_numeric()?),
            ))),
            ChannelPressureAmount { channel: Some(ch) } => Some(V::Plain(M::channel_pressure(
                *ch,
                denormalize_7_bit(feedback_value.to_numeric()?),
            ))),
            PitchBendChangeValue { channel: Some(ch) } => Some(V::Plain(M::pitch_bend_change(
                *ch,
                denormalize_14_bit_centered(feedback_value.to_numeric()?),
            ))),
            ControlChange14BitValue {
                channel: Some(ch),
                msb_controller_number: Some(mcn),
                ..
            } => Some(V::ControlChange14Bit(ControlChange14BitMessage::new(
                *ch,
                *mcn,
                denormalize_14_bit(feedback_value.to_numeric()?),
            ))),
            ParameterNumberValue {
                channel: Some(ch),
                number: Some(n),
                is_14_bit: Some(is_14_bit),
                is_registered: Some(is_registered),
                ..
            } => {
                let n = if !*is_registered && !*is_14_bit {
                    ParameterNumberMessage::non_registered_7_bit(
                        *ch,
                        *n,
                        denormalize_7_bit(feedback_value.to_numeric()?),
                    )
                } else if !*is_registered && *is_14_bit {
                    ParameterNumberMessage::non_registered_14_bit(
                        *ch,
                        *n,
                        denormalize_14_bit(feedback_value.to_numeric()?),
                    )
                } else if *is_registered && !*is_14_bit {
                    ParameterNumberMessage::registered_7_bit(
                        *ch,
                        *n,
                        denormalize_7_bit(feedback_value.to_numeric()?),
                    )
                } else if *is_registered && *is_14_bit {
                    ParameterNumberMessage::registered_14_bit(
                        *ch,
                        *n,
                        denormalize_14_bit(feedback_value.to_numeric()?),
                    )
                } else {
                    unreachable!()
                };
                Some(V::ParameterNumber(n))
            }
            Raw { pattern, .. } => {
                let raw_midi_event = pattern.to_concrete_midi_event(feedback_value.to_numeric()?);
                let address_info = RawFeedbackAddressInfo::Raw {
                    variable_range: pattern.variable_range(),
                };
                let value = V::Raw {
                    feedback_address_info: Some(address_info),
                    events: vec![raw_midi_event],
                };
                Some(value)
            }
            Script { script } => {
                let script = script.as_ref()?;
                // TODO-medium Make textual value available
                let events = script.execute(feedback_value.to_numeric()?).ok()?;
                let value = V::Raw {
                    feedback_address_info: None,
                    events,
                };
                Some(value)
            }
            Display { spec } => {
                let text = feedback_value.to_textual();
                let events: Vec<_> = match spec {
                    DisplaySpec::MackieLcd { scope } => {
                        let mut ascii_chars = filter_ascii_chars(&text);
                        scope
                            .lcd_portions()
                            .iter()
                            .filter_map(|range| {
                                let body = range
                                    .clone()
                                    .map(|_| ascii_chars.next().unwrap_or_default());
                                let sysex = mackie_lcd_sysex(0x14, range.start, body);
                                RawMidiEvent::try_from_iter(0, sysex).ok()
                            })
                            .collect()
                    }
                    DisplaySpec::SiniConE24 { scope } => {
                        let mut ascii_chars = filter_ascii_chars(&text);
                        scope
                            .destinations()
                            .iter()
                            .filter_map(move |dest| {
                                let line_length = dest.line_length();
                                let body = (0..line_length)
                                    .map(|_| ascii_chars.next().unwrap_or_default());
                                let white = (0xFF, 0xFF, 0xFF);
                                let sysex = sinicon_e24_sysex(
                                    1,
                                    dest.cell_index,
                                    dest.item_index,
                                    line_length,
                                    white,
                                    body,
                                );
                                RawMidiEvent::try_from_iter(0, sysex).ok()
                            })
                            .collect()
                    }
                    DisplaySpec::MackieSevenSegmentDisplay { scope } => {
                        // Reverse because we want right-aligned
                        let mut peekable_chars = text.chars().rev().peekable();
                        let mut codes = iter::from_fn(|| {
                            let ch = peekable_chars.next()?;
                            let next_ch = peekable_chars.peek().copied();
                            let result = convert_to_7_segment_code(ch, next_ch, true);
                            if result.consumed_one_more {
                                peekable_chars.next();
                            }
                            Some(result.code)
                        })
                        .flatten()
                        .fuse();
                        // Reverse because we want right-aligned
                        let positions = scope.positions();
                        let body = positions.iter().rev().flat_map(|pos| {
                            iter::once(0x40u8 + pos)
                                .chain(iter::once(codes.next().unwrap_or_default()))
                        });
                        let complete = mackie_7_segment_msg(body);
                        vec![RawMidiEvent::try_from_iter(0, complete).ok()?]
                    }
                };
                let feedback_info = RawFeedbackAddressInfo::Display { spec: *spec };
                let raw_value = V::Raw {
                    feedback_address_info: Some(feedback_info),
                    events,
                };
                Some(raw_value)
            }
            _ => None,
        }
    }

    /// Formats the given absolute control value.
    ///
    /// The formatting is done according to how source values of this source type usually look like.
    ///
    /// # Errors
    ///
    /// Returns an error if formatting values is not supported for this source type or if the
    /// control value type is not compatible with this source type.
    pub fn format_control_value(&self, value: ControlValue) -> Result<String, &'static str> {
        use MidiSource::*;
        let result = match self {
            ClockTempo => {
                let bpm = Bpm::from_unit_value(value.to_unit_value()?);
                format!("{:.2}", bpm.get())
            }
            ClockTransport { .. } => {
                return Err("clock transport sources have just one possible control value");
            }
            Script { .. } | Display { .. } => {
                format_percentage_without_unit(value.to_unit_value()?.get())
            }
            _ => self
                .convert_control_value_to_midi_value(value.to_unit_value()?)?
                .to_string(),
        };
        Ok(result)
    }

    /// Interprets the given text as MIDI value and returns the corresponding absolute control
    /// value.
    pub fn parse_control_value(&self, text: &str) -> Result<UnitValue, &'static str> {
        use MidiSource::*;
        let unit_value = match self {
            ClockTempo => {
                let bpm: Bpm = text.parse()?;
                bpm.to_unit_value()
            }
            ClockTransport { .. } => {
                return Err("parsing doesn't make sense for clock transport MIDI source");
            }
            Script { .. } | Display { .. } => parse_percentage_without_unit(text)?.try_into()?,
            _ => {
                let midi_value: i32 = text.parse().map_err(|_| "not a valid integer")?;
                self.convert_midi_value_to_control_value(midi_value)?
            }
        };
        Ok(unit_value)
    }

    /// Returns whether this source emits relative increments instead of absolute values.
    pub fn emits_increments(&self) -> bool {
        matches!(
            self,
            MidiSource::ControlChangeValue {
                custom_character, ..
            } | MidiSource::ControlChange14BitValue {
              custom_character, ..
            } | MidiSource::ParameterNumberValue {
              custom_character, ..
            } if custom_character.emits_increments()
        )
    }

    /// Converts the given absolute control value to an integer which reflects the MIDI source
    /// value.
    ///
    /// The returned integer is in most cases a 7-bit value. But it can also be 14-bit or even
    /// negative (if it's an increment). It depends on the source type.
    ///
    /// This method is intended for visualization purposes.
    ///
    /// # Errors
    ///
    /// Returns an error if it doesn't make sense for this source type (e.g. MIDI clock and sources
    /// which emit increments).
    fn convert_control_value_to_midi_value(&self, v: UnitValue) -> Result<i32, &'static str> {
        let value = AbsoluteValue::Continuous(v);
        use MidiSource::*;
        let midi_value: i32 = match self {
            NoteVelocity { .. }
            | NoteKeyNumber { .. }
            | PolyphonicKeyPressureAmount { .. }
            | ProgramChangeNumber { .. }
            | ChannelPressureAmount { .. }
            | ControlChangeValue { .. } => denormalize_7_bit(value),
            PitchBendChangeValue { .. } => denormalize_14_bit_centered::<i32>(value) - 8192,
            ControlChange14BitValue { .. } => denormalize_14_bit(value),
            ParameterNumberValue { is_14_bit, .. } => match *is_14_bit {
                None => return Err("not clear if 7- or 14-bit"),
                Some(is_14_bit) => {
                    if is_14_bit {
                        denormalize_14_bit(value)
                    } else {
                        denormalize_7_bit(value)
                    }
                }
            },
            Raw { pattern, .. } => v.to_discrete(pattern.max_discrete_value()) as _,
            ClockTempo | ClockTransport { .. } | Script { .. } | Display { .. } => {
                return Err("not supported");
            }
        };
        Ok(midi_value)
    }

    /// Like `convert_control_value_to_midi_value()` but in other direction.
    fn convert_midi_value_to_control_value(&self, value: i32) -> Result<UnitValue, &'static str> {
        use MidiSource::*;
        let unit_value = match self {
            NoteVelocity { .. }
            | NoteKeyNumber { .. }
            | PolyphonicKeyPressureAmount { .. }
            | ProgramChangeNumber { .. }
            | ChannelPressureAmount { .. } => {
                normalize_7_bit(U7::try_from(value).map_err(|_| "value not 7-bit")?)
            }
            ControlChangeValue { .. } => {
                normalize_7_bit(U7::try_from(value).map_err(|_| "value not 7-bit")?)
            }
            PitchBendChangeValue { .. } => normalize_14_bit_centered(
                U14::try_from(value + 8192).map_err(|_| "value not 14-bit")?,
            ),
            ControlChange14BitValue { .. } => {
                normalize_14_bit(U14::try_from(value).map_err(|_| "value not 14-bit")?)
            }
            ParameterNumberValue { is_14_bit, .. } => match *is_14_bit {
                None => return Err("not clear if 7- or 14-bit"),
                Some(is_14_bit) => {
                    if is_14_bit {
                        normalize_14_bit(U14::try_from(value).map_err(|_| "value not 14-bit")?)
                    } else {
                        normalize_7_bit(U7::try_from(value).map_err(|_| "value not 7-bit")?)
                    }
                }
            },
            Raw { pattern, .. } => {
                if value < 0 {
                    return Err("negative values not supported");
                }
                Fraction::new(value as _, pattern.max_discrete_value() as _)
            }
            ClockTempo | ClockTransport { .. } | Script { .. } | Display { .. } => {
                return Err("not supported");
            }
        };
        Ok(unit_value.to_unit_value())
    }

    pub fn max_discrete_value(&self) -> Option<u32> {
        use MidiSource::*;
        match self {
            NoteVelocity { .. }
            | PolyphonicKeyPressureAmount { .. }
            | ProgramChangeNumber { .. }
            | ChannelPressureAmount { .. }
            | NoteKeyNumber { .. } => Some(127),
            ControlChange14BitValue { .. } | PitchBendChangeValue { .. } => Some(16383),
            ControlChangeValue {
                custom_character, ..
            } => {
                if custom_character.emits_increments() {
                    None
                } else {
                    Some(127)
                }
            }
            ParameterNumberValue {
                custom_character,
                is_14_bit,
                ..
            } => {
                if custom_character.emits_increments() {
                    None
                } else if *is_14_bit == Some(true) {
                    Some(16383)
                } else {
                    Some(127)
                }
            }
            ClockTempo | ClockTransport { .. } | Script { .. } | Display { .. } => None,
            Raw {
                custom_character,
                pattern,
            } => {
                if custom_character.emits_increments() {
                    None
                } else {
                    Some(pattern.max_discrete_value() as _)
                }
            }
        }
    }
}

fn matches<T: PartialEq + Eq>(actual_value: T, configured_value: Option<T>) -> bool {
    match configured_value {
        None => true,
        Some(v) => actual_value == v,
    }
}

fn calc_control_value_from_n_bit_cc<T: Into<u32>>(
    character: SourceCharacter,
    cc_control_value: T,
    resolution: u32,
) -> Result<ControlValue, &'static str> {
    use SourceCharacter::*;
    let cc_control_value = cc_control_value.into();
    let result = match character {
        RangeElement | MomentaryButton => abs(normalize_n_bit(cc_control_value, resolution)),
        Encoder1 | Encoder2 | Encoder3 => {
            let value_7_bit = extract_low_7_bit(cc_control_value);
            let increment = match character {
                Encoder1 => DiscreteIncrement::from_encoder_1_value(value_7_bit)?,
                Encoder2 => DiscreteIncrement::from_encoder_2_value(value_7_bit)?,
                Encoder3 => DiscreteIncrement::from_encoder_3_value(value_7_bit)?,
                _ => unreachable!("impossible"),
            };
            rel(increment)
        }
        ToggleButton => abs(max_n_bit_fraction(resolution)),
    };
    Ok(result)
}

const MIN_U7_FRACTION: Fraction = Fraction::new_min(U7::MAX.get() as _);

fn normalize_7_bit<T: Into<u32>>(value: T) -> Fraction {
    normalize_n_bit(value, 7)
}

fn normalize_14_bit(value: U14) -> Fraction {
    normalize_n_bit(value, 14)
}

fn normalize_n_bit<T: Into<u32>>(value: T, resolution: u32) -> Fraction {
    Fraction::new(value.into(), 2u32.pow(resolution) - 1)
}

fn max_n_bit_fraction(resolution: u32) -> Fraction {
    Fraction::new_max(2u32.pow(resolution) - 1)
}

/// See denormalize_14_bit_centered for an explanation
fn normalize_14_bit_centered(value: U14) -> Fraction {
    if value == U14::MAX {
        return Fraction::new_max(U14::MAX.into());
    }
    Fraction::new(value.into(), U14::MAX.get() as u32 + 1)
}

fn denormalize_7_bit<T: From<U7>>(value: AbsoluteValue) -> T {
    match value {
        AbsoluteValue::Continuous(v) => {
            unsafe { U7::new_unchecked((v.get() * U7::MAX.get() as f64).round() as u8) }.into()
        }
        AbsoluteValue::Discrete(f) => U7::try_from(f.actual()).unwrap_or(U7::MAX).into(),
    }
}

fn denormalize_14_bit<T: From<U14>>(value: AbsoluteValue) -> T {
    match value {
        AbsoluteValue::Continuous(v) => {
            unsafe { U14::new_unchecked((v.get() * U14::MAX.get() as f64).round() as u16) }.into()
        }
        AbsoluteValue::Discrete(f) => U14::try_from(f.actual()).unwrap_or(U14::MAX).into(),
    }
}

/// When doing the mapping, this doesn't consider 16383 as maximum value but 16384. However, the
/// result is clamped again to (0..=16383) ... it's a bit like using `ceil()` instead of `round()`.
/// The intended effect is that now the range has a discrete center, which it normally doesn't have
/// because there's an even number of possible values. This way of denormalization is intended to be
/// used with controllers that are known to be centered. However, as you can imagine, there's also
/// a side effect: Instead of having an equal distribution, this algorithm slightly favors higher
/// numbers, so ideally it really should only be used for centered controllers.
///
/// Take pitch bend change as an example: The official center of pitch bend is considered to be the
/// ceiling of the actual center value, which is 8192.
///
/// - Unsigned view: Possible pitch bend values go from 0 to 16383. Exact center would be 8191.5.
///   Official center is 8192.
/// - Signed view: Possible pitch bend values go from -8192 to 8191. Exact center would be -0.5.
///   Official center is 0.
fn denormalize_14_bit_centered<T: From<U14>>(value: AbsoluteValue) -> T {
    match value {
        AbsoluteValue::Continuous(v) => {
            let spread = (v.get() * (U14::MAX.get() + 1) as f64).round() as u16;
            unsafe { U14::new_unchecked(spread.min(16383)) }.into()
        }
        AbsoluteValue::Discrete(f) => U14::try_from(f.actual()).unwrap_or(U14::MAX).into(),
    }
}

const fn abs(value: Fraction) -> ControlValue {
    ControlValue::AbsoluteDiscrete(value)
}

const fn rel(increment: DiscreteIncrement) -> ControlValue {
    ControlValue::Relative(increment)
}

fn extract_low_7_bit<T: Into<u32>>(value: T) -> U7 {
    U7::new((value.into() & 0x7f) as u8)
}

fn mackie_lcd_sysex(
    model_id: u8,
    display_offset: u8,
    body: impl Iterator<Item = u8>,
) -> impl Iterator<Item = u8> {
    let start = it([0xF0, 0x00, 0x00, 0x66, model_id, 0x12, display_offset]);
    start.chain(body).chain(end())
}

fn it<const N: usize>(arr: [u8; N]) -> impl Iterator<Item = u8> {
    IntoIterator::into_iter(arr)
}

fn end() -> impl Iterator<Item = u8> {
    iter::once(0xF7)
}

fn sinicon_e24_sysex(
    controller_number: u8,
    cell_index: u8,
    item_index: u8,
    line_length: u8,
    (r, g, b): (u8, u8, u8),
    body: impl Iterator<Item = u8>,
) -> impl Iterator<Item = u8> {
    // Display type 1 (4 lines)
    let display_type = 1;
    // Item type 1 (text)
    let item_type = 1;
    // Item style (0 means background color, anything else means line length)
    let item_style = line_length;
    // Wildcard (text color)
    let wildcard = 0;
    let start = it([
        0xF0,
        0x00,
        0x02,
        0x38,
        controller_number,
        cell_index + 1,
        display_type,
        item_index + 1,
        item_type,
        item_style,
        wildcard,
        r,
        g,
        b,
    ]);
    start.chain(body).chain(end())
}

fn mackie_7_segment_msg(body: impl Iterator<Item = u8>) -> impl Iterator<Item = u8> {
    iter::once(0xB0).chain(body)
}

fn filter_ascii_chars(text: &str) -> impl Iterator<Item = u8> + '_ {
    text.chars()
        .filter_map(|ch| if ch.is_ascii() { Some(ch as u8) } else { None })
        .fuse()
}

#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    Debug,
    IntoEnumIterator,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(usize)]
pub enum DisplayType {
    #[cfg_attr(feature = "serde", serde(rename = "mackie-lcd"))]
    #[display(fmt = "Mackie LCD")]
    MackieLcd,
    #[cfg_attr(feature = "serde", serde(rename = "mackie-seven"))]
    #[display(fmt = "Mackie 7-segment display")]
    MackieSevenSegmentDisplay,
    #[cfg_attr(feature = "serde", serde(rename = "sinicon-e24"))]
    #[display(fmt = "SiniCon E24")]
    SiniConE24,
}

impl DisplayType {
    pub fn display_count(self) -> u8 {
        use DisplayType::*;
        match self {
            MackieLcd => MackieLcdScope::CHANNEL_COUNT,
            SiniConE24 => SiniConE24Scope::CELL_COUNT,
            // Not handled in the usual way
            MackieSevenSegmentDisplay => 0,
        }
    }

    pub fn line_count(self) -> u8 {
        use DisplayType::*;
        match self {
            MackieLcd => MackieLcdScope::LINE_COUNT,
            MackieSevenSegmentDisplay => 1,
            SiniConE24 => SiniConE24Scope::ITEM_COUNT,
        }
    }
}

impl Default for DisplayType {
    fn default() -> Self {
        DisplayType::MackieLcd
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum DisplaySpec {
    MackieLcd {
        scope: MackieLcdScope,
    },
    MackieSevenSegmentDisplay {
        scope: MackieSevenSegmentDisplayScope,
    },
    SiniConE24 {
        scope: SiniConE24Scope,
    },
}

#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    Debug,
    IntoEnumIterator,
    TryFromPrimitive,
    IntoPrimitive,
    Display,
    Serialize_repr,
    Deserialize_repr,
)]
#[repr(usize)]
pub enum MackieSevenSegmentDisplayScope {
    #[display(fmt = "<All>")]
    All = 0,
    #[display(fmt = "Assignment")]
    Assignment = 1,
    #[display(fmt = "Time code")]
    Tc = 2,
    #[display(fmt = ".... Hours/bars (3)")]
    TcLeft3Digits = 3,
    #[display(fmt = ".... Minutes/beats (2)")]
    TcLeft2Digits = 4,
    #[display(fmt = ".... Seconds/sub (2)")]
    TcRight2Digits = 5,
    #[display(fmt = ".... Frames/ticks (3)")]
    TcRight3Digits = 6,
}

impl MackieSevenSegmentDisplayScope {
    pub fn positions(&self) -> DisplayPositions {
        use MackieSevenSegmentDisplayScope::*;
        let positions = match self {
            All => vec![0x0B, 0x0A, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
            Assignment => vec![0x0B, 0x0A],
            Tc => vec![9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
            TcLeft3Digits => vec![9, 8, 7],
            TcLeft2Digits => vec![6, 5],
            TcRight2Digits => vec![4, 3],
            TcRight3Digits => vec![2, 1, 0],
        };
        DisplayPositions::new(positions)
    }
}

impl Default for MackieSevenSegmentDisplayScope {
    fn default() -> Self {
        MackieSevenSegmentDisplayScope::All
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct MackieLcdScope {
    pub channel: Option<u8>,
    pub line: Option<u8>,
}

impl MackieLcdScope {
    const CHANNEL_LEN: u8 = 7;
    const CHANNEL_COUNT: u8 = 8;
    const LINE_COUNT: u8 = 2;
    const LINE_LEN: u8 = Self::CHANNEL_COUNT * Self::CHANNEL_LEN;

    pub fn new(channel: Option<u8>, line: Option<u8>) -> Self {
        Self {
            channel: channel.map(|ch| ch.min(Self::CHANNEL_COUNT - 1)),
            line: line.map(|l| l.min(Self::LINE_COUNT - 1)),
        }
    }

    pub fn lcd_portions(&self) -> LcdPortions {
        fn range(start: u8, len: u8) -> Range<u8> {
            start..(start + len)
        }
        let ranges = match (self.channel, self.line) {
            (None, None) => vec![range(0, Self::LINE_COUNT * Self::LINE_LEN)],
            (None, Some(l)) => vec![range(l * Self::LINE_LEN, Self::LINE_LEN)],
            (Some(ch), None) => (0..Self::LINE_COUNT)
                .map(|l| {
                    range(
                        l * Self::LINE_LEN + ch * Self::CHANNEL_LEN,
                        Self::CHANNEL_LEN,
                    )
                })
                .collect(),
            (Some(ch), Some(l)) => vec![range(
                l * Self::LINE_LEN + ch * Self::CHANNEL_LEN,
                Self::CHANNEL_LEN,
            )],
        };
        LcdPortions::new(ranges)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct SiniConE24Scope {
    pub cell_index: Option<u8>,
    pub item_index: Option<u8>,
}

impl SiniConE24Scope {
    const CELL_COUNT: u8 = 24;
    const ITEM_COUNT: u8 = 4;

    pub fn new(cell_index: Option<u8>, item_index: Option<u8>) -> Self {
        Self {
            cell_index: cell_index.map(|c| c.min(Self::CELL_COUNT - 1)),
            item_index: item_index.map(|i| i.min(Self::ITEM_COUNT - 1)),
        }
    }

    pub fn destinations(&self) -> Vec<SiniConE24Destination> {
        match (self.cell_index, self.item_index) {
            (None, None) => (0..Self::CELL_COUNT)
                .flat_map(|c| (0..Self::ITEM_COUNT).map(move |i| SiniConE24Destination::new(c, i)))
                .collect(),
            (None, Some(i)) => (0..Self::CELL_COUNT)
                .map(|c| SiniConE24Destination::new(c, i))
                .collect(),
            (Some(c), None) => (0..Self::ITEM_COUNT)
                .map(|i| SiniConE24Destination::new(c, i))
                .collect(),
            (Some(c), Some(i)) => vec![SiniConE24Destination::new(c, i)],
        }
    }
}

/// A sequence of positions on a display, left-to-right.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DisplayPositions {
    positions: Vec<u8>,
}

impl DisplayPositions {
    /// Takes left-to-right display positions.
    pub fn new(positions: Vec<u8>) -> Self {
        Self { positions }
    }

    /// Returns left-to-right display positions.
    pub fn iter(&self) -> impl Iterator<Item = u8> + DoubleEndedIterator + '_ {
        self.positions.iter().copied()
    }
}

/// A list of disjoint position intervals on a display, each one left-to-right.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct LcdPortions {
    ranges: Vec<Range<u8>>,
}

impl LcdPortions {
    pub fn new(ranges: Vec<Range<u8>>) -> Self {
        Self { ranges }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Range<u8>> + '_ {
        self.ranges.iter()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SiniConE24Destination {
    pub cell_index: u8,
    pub item_index: u8,
}

impl SiniConE24Destination {
    pub fn new(cell_index: u8, item_index: u8) -> Self {
        Self {
            cell_index,
            item_index,
        }
    }

    pub fn line_length(&self) -> u8 {
        match self.item_index {
            0 | 1 | 2 => 16,
            3 => 9,
            _ => 0,
        }
    }
}

/// `reverse` must be used if we are iterating over the text in reverse order (for right alignment).
fn convert_to_7_segment_code(ch: char, next_ch: Option<char>, reverse: bool) -> ConversionResult {
    let (ch, with_decimal_point) = if reverse {
        if ch == '.' {
            (next_ch.unwrap_or(' '), true)
        } else {
            (ch, false)
        }
    } else {
        (ch, next_ch == Some('.'))
    };
    if ch == '.' {
        // Translate period to space with decimal point, not to underscore.
        return ConversionResult {
            code: Some(0x20 + 0x40),
            consumed_one_more: false,
        };
    }
    if !ch.is_ascii() {
        return Default::default();
    }
    let ch = ch.to_ascii_uppercase() as u8;
    let res = match ch {
        b':' | b'!' | b'@' => return Default::default(),
        b'@'..=b'`' => ch - 0x40,
        b' '..=b'?' => ch,
        _ => {
            return Default::default();
        }
    };
    if with_decimal_point {
        ConversionResult {
            code: Some(res + 0x40),
            consumed_one_more: true,
        }
    } else {
        ConversionResult {
            code: Some(res),
            consumed_one_more: false,
        }
    }
}

#[derive(Default)]
struct ConversionResult {
    code: Option<u8>,
    consumed_one_more: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::test_util::TestMidiSourceScript;
    use approx::*;
    use helgoboss_midi::test_util::{channel as ch, controller_number as cn, key_number as kn, *};
    use helgoboss_midi::RawShortMessage;

    type TestMidiSource = MidiSource<TestMidiSourceScript>;

    #[test]
    fn note_velocity_1() {
        // Given
        let source = TestMidiSource::NoteVelocity {
            channel: Some(ch(0)),
            key_number: None,
        };
        // When
        // Then
        assert_abs_diff_eq!(
            source
                .control(&plain(note_on(0, 64, 127,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source.control(&plain(note_on(0, 64, 127,))).unwrap(),
            frac(127, 127)
        );
        assert_abs_diff_eq!(
            source
                .control(&plain(note_on(0, 20, 0,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.0)
        );
        assert_eq!(
            source.control(&plain(note_on(0, 20, 0,))).unwrap(),
            frac(0, 127)
        );
        assert_abs_diff_eq!(
            source
                .control(&plain(note_off(0, 20, 100,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.0)
        );
        assert_eq!(
            source.control(&plain(note_off(0, 20, 100,))).unwrap(),
            frac(0, 127)
        );
        assert_eq!(source.control(&plain(note_on(4, 20, 0,))), None);
        assert_eq!(source.control(&plain(control_change(3, 64, 127,))), None);
        assert_eq!(source.control(&plain(program_change(5, 64,))), None);
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(3, 14, 64,))),
            None
        );
        assert_eq!(source.control(&plain(channel_pressure(3, 79,))), None);
        assert_eq!(source.control(&plain(pitch_bend_change(3, 15012,))), None);
        assert_eq!(source.control(&plain(timing_clock())), None);
        assert_eq!(source.control(&plain(start())), None);
        assert_eq!(source.control(&plain(r#continue())), None);
        assert_eq!(source.control(&plain(stop())), None);
        assert_eq!(source.control(&plain(active_sensing())), None);
        assert_eq!(source.control(&plain(system_reset())), None);
        assert_eq!(source.control(&pn(rpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(nrpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(rpn(1, 342, 45))), None);
        assert_eq!(source.control(&pn(nrpn(1, 520, 24))), None);
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 12000))),
            None
        );
        assert_eq!(source.control(&tempo(120.0)), None);
        assert_eq!(source.feedback::<RawShortMessage>(fv(0.5)), None);
        assert_eq!(
            source.format_control_value(abs(0.5)).expect("bad").as_str(),
            "64"
        );
    }

    #[test]
    fn note_velocity_2() {
        // Given
        let source = TestMidiSource::NoteVelocity {
            channel: Some(ch(4)),
            key_number: Some(kn(20)),
        };
        // When
        // Then
        assert_eq!(source.control(&plain(note_on(0, 64, 127,))), None);
        assert_abs_diff_eq!(
            source
                .control(&plain(note_on(4, 20, 0,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.0)
        );
        assert_eq!(
            source.control(&plain(note_on(4, 20, 0,))).unwrap(),
            frac(0, 127)
        );
        assert_eq!(source.control(&plain(note_off(15, 20, 100,))), None);
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.5)),
            Some(plain(note_on(4, 20, 64)))
        );
    }

    #[test]
    fn note_key_number_1() {
        // Given
        let source = TestMidiSource::NoteKeyNumber { channel: None };
        // When
        // Then
        assert_abs_diff_eq!(
            source
                .control(&plain(note_on(0, 127, 55,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source.control(&plain(note_on(0, 127, 55,))).unwrap(),
            frac(127, 127)
        );
        assert_abs_diff_eq!(
            source
                .control(&plain(note_on(1, 0, 64,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.0)
        );
        assert_eq!(
            source.control(&plain(note_on(1, 0, 64,))).unwrap(),
            frac(0, 127)
        );
        assert_eq!(source.control(&plain(note_off(0, 20, 100,))), None);
        assert_eq!(source.control(&plain(note_on(4, 20, 0,))), None);
        assert_eq!(source.control(&plain(control_change(3, 64, 127,))), None);
        assert_eq!(source.control(&plain(program_change(5, 64,))), None);
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(3, 14, 64,))),
            None
        );
        assert_eq!(source.control(&plain(channel_pressure(3, 79,))), None);
        assert_eq!(source.control(&plain(pitch_bend_change(3, 15012,))), None);
        assert_eq!(source.control(&plain(timing_clock())), None);
        assert_eq!(source.control(&plain(start())), None);
        assert_eq!(source.control(&plain(r#continue())), None);
        assert_eq!(source.control(&plain(stop())), None);
        assert_eq!(source.control(&plain(active_sensing())), None);
        assert_eq!(source.control(&plain(system_reset())), None);
        assert_eq!(source.control(&pn(rpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(nrpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(rpn(1, 342, 45))), None);
        assert_eq!(source.control(&pn(nrpn(1, 520, 24))), None);
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 12000))),
            None
        );
        assert_eq!(source.control(&tempo(120.0)), None);
        assert_eq!(source.feedback::<RawShortMessage>(fv(0.5)), None);
        assert_eq!(
            source.format_control_value(abs(0.5)).expect("bad").as_str(),
            "64"
        );
    }

    #[test]
    fn note_key_number_2() {
        // Given
        let source = TestMidiSource::NoteKeyNumber {
            channel: Some(ch(1)),
        };
        // When
        // Then
        assert_eq!(source.control(&plain(note_on(0, 127, 55,))), None);
        assert_abs_diff_eq!(
            source
                .control(&plain(note_on(1, 0, 64,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.0)
        );
        assert_eq!(
            source.control(&plain(note_on(1, 0, 64,))).unwrap(),
            frac(0, 127)
        );
        assert_eq!(source.control(&plain(note_off(0, 20, 100,))), None);
        assert_eq!(source.control(&plain(note_on(4, 20, 0,))), None);
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.5)),
            Some(plain(note_on(1, 64, 127)))
        );
    }

    #[test]
    fn polyphonic_key_pressure_amount_1() {
        // Given
        let source = TestMidiSource::PolyphonicKeyPressureAmount {
            channel: Some(ch(1)),
            key_number: None,
        };
        // When
        // Then
        assert_eq!(source.control(&plain(note_on(0, 127, 55,))), None);
        assert_eq!(source.control(&plain(note_on(1, 0, 64,))), None);
        assert_eq!(source.control(&plain(note_off(0, 20, 100,))), None);
        assert_eq!(source.control(&plain(note_on(4, 20, 0,))), None);
        assert_eq!(source.control(&plain(control_change(3, 64, 127,))), None);
        assert_eq!(source.control(&plain(program_change(5, 64,))), None);
        assert_abs_diff_eq!(
            source
                .control(&plain(polyphonic_key_pressure(1, 14, 127,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source
                .control(&plain(polyphonic_key_pressure(1, 14, 127,)))
                .unwrap(),
            frac(127, 127)
        );
        assert_abs_diff_eq!(
            source
                .control(&plain(polyphonic_key_pressure(1, 16, 0,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.0)
        );
        assert_eq!(
            source
                .control(&plain(polyphonic_key_pressure(1, 16, 0,)))
                .unwrap(),
            frac(0, 127)
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(3, 14, 127))),
            None
        );
        assert_eq!(source.control(&plain(channel_pressure(3, 79,))), None);
        assert_eq!(source.control(&plain(pitch_bend_change(3, 15012,))), None);
        assert_eq!(source.control(&plain(timing_clock())), None);
        assert_eq!(source.control(&plain(start())), None);
        assert_eq!(source.control(&plain(r#continue())), None);
        assert_eq!(source.control(&plain(stop())), None);
        assert_eq!(source.control(&plain(active_sensing())), None);
        assert_eq!(source.control(&plain(system_reset())), None);
        assert_eq!(source.control(&pn(rpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(nrpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(rpn(1, 342, 45))), None);
        assert_eq!(source.control(&pn(nrpn(1, 520, 24))), None);
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 12000))),
            None
        );
        assert_eq!(source.control(&tempo(120.0)), None);
        assert_eq!(source.feedback::<RawShortMessage>(fv(0.5)), None);
        assert_eq!(
            source.format_control_value(abs(0.5)).expect("bad").as_str(),
            "64"
        );
    }

    #[test]
    fn polyphonic_key_pressure_amount_2() {
        // Given
        let source = TestMidiSource::PolyphonicKeyPressureAmount {
            channel: Some(ch(1)),
            key_number: Some(kn(53)),
        };
        // When
        // Then
        assert_abs_diff_eq!(
            source
                .control(&plain(polyphonic_key_pressure(1, 53, 127,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source
                .control(&plain(polyphonic_key_pressure(1, 53, 127,)))
                .unwrap(),
            frac(127, 127)
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 16, 0,))),
            None
        );
        assert_eq!(source.control(&plain(channel_pressure(3, 79,))), None);
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.5)),
            Some(plain(polyphonic_key_pressure(1, 53, 64)))
        );
    }

    #[test]
    fn control_change_value_1() {
        // Given
        let source = TestMidiSource::ControlChangeValue {
            channel: Some(ch(1)),
            controller_number: None,
            custom_character: SourceCharacter::RangeElement,
        };
        // When
        // Then
        assert_eq!(source.control(&plain(note_on(0, 127, 55,))), None);
        assert_eq!(source.control(&plain(note_on(1, 0, 64,))), None);
        assert_eq!(source.control(&plain(note_off(0, 20, 100,))), None);
        assert_eq!(source.control(&plain(note_on(4, 20, 0,))), None);
        assert_eq!(source.control(&plain(control_change(3, 64, 127,))), None);
        assert_abs_diff_eq!(
            source
                .control(&plain(control_change(1, 64, 127,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source.control(&plain(control_change(1, 64, 127,))).unwrap(),
            frac(127, 127)
        );
        assert_eq!(source.control(&plain(program_change(5, 64,))), None);
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 53, 127,))),
            None
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 16, 0,))),
            None
        );
        assert_eq!(source.control(&plain(channel_pressure(3, 79,))), None);
        assert_eq!(source.control(&plain(pitch_bend_change(3, 15012,))), None);
        assert_eq!(source.control(&plain(timing_clock())), None);
        assert_eq!(source.control(&plain(start())), None);
        assert_eq!(source.control(&plain(r#continue())), None);
        assert_eq!(source.control(&plain(stop())), None);
        assert_eq!(source.control(&plain(active_sensing())), None);
        assert_eq!(source.control(&plain(system_reset())), None);
        assert_eq!(source.control(&pn(rpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(nrpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(rpn(1, 342, 45))), None);
        assert_eq!(source.control(&pn(nrpn(1, 520, 24))), None);
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 12000))),
            None
        );
        assert_eq!(source.control(&tempo(120.0)), None);
        assert_eq!(source.feedback::<RawShortMessage>(fv(0.5)), None);
        assert_eq!(
            source.format_control_value(abs(0.5)).expect("bad").as_str(),
            "64"
        );
    }

    #[test]
    fn control_change_value_2() {
        // Given
        let source = TestMidiSource::ControlChangeValue {
            channel: Some(ch(1)),
            controller_number: Some(cn(64)),
            custom_character: SourceCharacter::Encoder2,
        };
        // When
        // Then
        assert_eq!(source.control(&plain(control_change(1, 65, 127,))), None);
        assert_abs_diff_eq!(
            source.control(&plain(control_change(1, 64, 62,))).unwrap(),
            rel(-2)
        );
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 12000))),
            None
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.0)),
            Some(plain(control_change(1, 64, 0)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.25)),
            Some(plain(control_change(1, 64, 32)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.5)),
            Some(plain(control_change(1, 64, 64)))
        );
        // In a center-oriented mapping this would yield 96 instead of 95
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.75)),
            Some(plain(control_change(1, 64, 95)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(1.0)),
            Some(plain(control_change(1, 64, 127)))
        );
    }

    #[test]
    fn program_change_number_1() {
        // Given
        let source = TestMidiSource::ProgramChangeNumber { channel: None };
        // When
        // Then
        assert_eq!(source.control(&plain(note_on(0, 127, 55,))), None);
        assert_eq!(source.control(&plain(note_on(1, 0, 64,))), None);
        assert_eq!(source.control(&plain(note_off(0, 20, 100,))), None);
        assert_eq!(source.control(&plain(note_on(4, 20, 0,))), None);
        assert_eq!(source.control(&plain(control_change(3, 64, 127,))), None);
        assert_eq!(source.control(&plain(control_change(1, 64, 127,))), None);
        assert_abs_diff_eq!(
            source
                .control(&plain(program_change(5, 0,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.0)
        );
        assert_eq!(
            source.control(&plain(program_change(5, 0,))).unwrap(),
            frac(0, 127)
        );
        assert_abs_diff_eq!(
            source
                .control(&plain(program_change(6, 127,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source.control(&plain(program_change(6, 127,))).unwrap(),
            frac(127, 127)
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 53, 127,))),
            None
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 16, 0,))),
            None
        );
        assert_eq!(source.control(&plain(channel_pressure(3, 79,))), None);
        assert_eq!(source.control(&plain(pitch_bend_change(3, 15012,))), None);
        assert_eq!(source.control(&plain(timing_clock())), None);
        assert_eq!(source.control(&plain(start())), None);
        assert_eq!(source.control(&plain(r#continue())), None);
        assert_eq!(source.control(&plain(stop())), None);
        assert_eq!(source.control(&plain(active_sensing())), None);
        assert_eq!(source.control(&plain(system_reset())), None);
        assert_eq!(source.control(&pn(rpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(nrpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(rpn(1, 342, 45))), None);
        assert_eq!(source.control(&pn(nrpn(1, 520, 24))), None);
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 12000))),
            None
        );
        assert_eq!(source.control(&tempo(120.0)), None);
        assert_eq!(source.feedback::<RawShortMessage>(fv(0.5)), None);
        assert_eq!(
            source.format_control_value(abs(0.5)).expect("bad").as_str(),
            "64"
        );
    }

    #[test]
    fn program_change_number_2() {
        // Given
        let source = TestMidiSource::ProgramChangeNumber {
            channel: Some(ch(10)),
        };
        // When
        // Then
        assert_abs_diff_eq!(
            source
                .control(&plain(program_change(10, 0,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.0)
        );
        assert_eq!(
            source.control(&plain(program_change(10, 0,))).unwrap(),
            frac(0, 127)
        );
        assert_eq!(source.control(&plain(program_change(6, 127,))), None);
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.5)),
            Some(plain(program_change(10, 64)))
        );
    }

    #[test]
    fn channel_pressure_amount_1() {
        // Given
        let source = TestMidiSource::ChannelPressureAmount { channel: None };
        // When
        // Then
        assert_eq!(source.control(&plain(note_on(0, 127, 55,))), None);
        assert_eq!(source.control(&plain(note_on(1, 0, 64,))), None);
        assert_eq!(source.control(&plain(note_off(0, 20, 100,))), None);
        assert_eq!(source.control(&plain(note_on(4, 20, 0,))), None);
        assert_eq!(source.control(&plain(control_change(3, 64, 127,))), None);
        assert_eq!(source.control(&plain(control_change(1, 64, 127,))), None);
        assert_abs_diff_eq!(
            source
                .control(&plain(channel_pressure(5, 0,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.0)
        );
        assert_eq!(
            source.control(&plain(channel_pressure(5, 0,))).unwrap(),
            frac(0, 127)
        );
        assert_abs_diff_eq!(
            source
                .control(&plain(channel_pressure(6, 127,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source.control(&plain(channel_pressure(6, 127,))).unwrap(),
            frac(127, 127)
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 53, 127,))),
            None
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 16, 0,))),
            None
        );
        assert_eq!(source.control(&plain(program_change(3, 79,))), None);
        assert_eq!(source.control(&plain(pitch_bend_change(3, 15012,))), None);
        assert_eq!(source.control(&plain(timing_clock())), None);
        assert_eq!(source.control(&plain(start())), None);
        assert_eq!(source.control(&plain(r#continue())), None);
        assert_eq!(source.control(&plain(stop())), None);
        assert_eq!(source.control(&plain(active_sensing())), None);
        assert_eq!(source.control(&plain(system_reset())), None);
        assert_eq!(source.control(&pn(rpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(nrpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(rpn(1, 342, 45))), None);
        assert_eq!(source.control(&pn(nrpn(1, 520, 24))), None);
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 12000))),
            None
        );
        assert_eq!(source.control(&tempo(120.0)), None);
        assert_eq!(source.feedback::<RawShortMessage>(fv(0.5)), None);
        assert_eq!(
            source.format_control_value(abs(0.5)).expect("bad").as_str(),
            "64"
        );
    }

    #[test]
    fn channel_pressure_amount_2() {
        // Given
        let source = TestMidiSource::ChannelPressureAmount {
            channel: Some(ch(15)),
        };
        // When
        // Then
        assert_eq!(source.control(&plain(channel_pressure(5, 0,))), None);
        assert_abs_diff_eq!(
            source
                .control(&plain(channel_pressure(15, 127,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source.control(&plain(channel_pressure(15, 127,))).unwrap(),
            frac(127, 127)
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 53, 127,))),
            None
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 16, 0,))),
            None
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.5)),
            Some(plain(channel_pressure(15, 64)))
        );
    }

    #[test]
    fn pitch_bend_change_value_1() {
        // Given
        let source = TestMidiSource::PitchBendChangeValue { channel: None };
        // When
        // Then
        assert_eq!(source.control(&plain(note_on(0, 127, 55,))), None);
        assert_eq!(source.control(&plain(note_on(1, 0, 64,))), None);
        assert_eq!(source.control(&plain(note_off(0, 20, 100,))), None);
        assert_eq!(source.control(&plain(note_on(4, 20, 0,))), None);
        assert_eq!(source.control(&plain(control_change(3, 64, 127,))), None);
        assert_eq!(source.control(&plain(control_change(1, 64, 127,))), None);
        assert_abs_diff_eq!(
            source
                .control(&plain(pitch_bend_change(5, 0,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.0)
        );
        assert_eq!(
            source.control(&plain(pitch_bend_change(5, 0,))).unwrap(),
            frac(0, 16384)
        );
        assert_abs_diff_eq!(
            source
                .control(&plain(pitch_bend_change(6, 4096,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.25)
        );
        assert_eq!(
            source.control(&plain(pitch_bend_change(6, 4096,))).unwrap(),
            frac(4096, 16384)
        );
        assert_abs_diff_eq!(
            source
                .control(&plain(pitch_bend_change(6, 8192,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.5)
        );
        assert_eq!(
            source.control(&plain(pitch_bend_change(6, 8192,))).unwrap(),
            frac(8192, 16384)
        );
        assert_abs_diff_eq!(
            source
                .control(&plain(pitch_bend_change(6, 12288,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.75)
        );
        assert_eq!(
            source
                .control(&plain(pitch_bend_change(6, 12288,)))
                .unwrap(),
            frac(12288, 16384)
        );
        assert_abs_diff_eq!(
            source
                .control(&plain(pitch_bend_change(6, 16383,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source
                .control(&plain(pitch_bend_change(6, 16383,)))
                .unwrap(),
            frac(16383, 16383)
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 53, 127,))),
            None
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 16, 0,))),
            None
        );
        assert_eq!(source.control(&plain(program_change(3, 79,))), None);
        assert_eq!(source.control(&plain(channel_pressure(3, 2,))), None);
        assert_eq!(source.control(&plain(timing_clock())), None);
        assert_eq!(source.control(&plain(start())), None);
        assert_eq!(source.control(&plain(r#continue())), None);
        assert_eq!(source.control(&plain(stop())), None);
        assert_eq!(source.control(&plain(active_sensing())), None);
        assert_eq!(source.control(&plain(system_reset())), None);
        assert_eq!(source.control(&pn(rpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(nrpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(rpn(1, 342, 45))), None);
        assert_eq!(source.control(&pn(nrpn(1, 520, 24))), None);
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 12000))),
            None
        );
        assert_eq!(source.control(&tempo(120.0)), None);
        assert_eq!(source.feedback::<RawShortMessage>(fv(0.5)), None);
        assert_eq!(
            source.format_control_value(abs(0.5)).expect("bad").as_str(),
            "0"
        );
    }

    #[test]
    fn pitch_bend_change_value_2() {
        // Given
        let source = TestMidiSource::PitchBendChangeValue {
            channel: Some(ch(3)),
        };
        // When
        // Then
        assert_abs_diff_eq!(
            source
                .control(&plain(pitch_bend_change(3, 0,)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.0)
        );
        assert_eq!(
            source.control(&plain(pitch_bend_change(3, 0,))).unwrap(),
            frac(0, 16384)
        );
        assert_eq!(source.control(&plain(pitch_bend_change(6, 8192,))), None);
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.0)),
            Some(plain(pitch_bend_change(3, 0)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.25)),
            Some(plain(pitch_bend_change(3, 4096)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.5)),
            Some(plain(pitch_bend_change(3, 8192)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.75)),
            Some(plain(pitch_bend_change(3, 12288)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(1.0)),
            Some(plain(pitch_bend_change(3, 16383)))
        );
    }

    #[test]
    fn control_change_14_bit_value_1() {
        // Given
        let source = TestMidiSource::ControlChange14BitValue {
            channel: Some(ch(1)),
            msb_controller_number: None,
            custom_character: Default::default(),
        };
        // When
        // Then
        assert_eq!(source.control(&plain(note_on(0, 127, 55,))), None);
        assert_eq!(source.control(&plain(note_on(1, 0, 64,))), None);
        assert_eq!(source.control(&plain(note_off(0, 20, 100,))), None);
        assert_eq!(source.control(&plain(note_on(4, 20, 0,))), None);
        assert_eq!(source.control(&plain(control_change(3, 64, 127,))), None);
        assert_eq!(source.control(&plain(control_change(1, 64, 127,))), None);
        assert_abs_diff_eq!(
            source
                .control(&cc(control_change_14_bit(1, 10, 4096)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.2500152597204419)
        );
        assert_eq!(
            source
                .control(&cc(control_change_14_bit(1, 10, 4096)))
                .unwrap(),
            frac(4096, 16383)
        );
        assert_abs_diff_eq!(
            source
                .control(&cc(control_change_14_bit(1, 10, 16383)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source
                .control(&cc(control_change_14_bit(1, 10, 16383)))
                .unwrap(),
            frac(16383, 16383)
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 53, 127,))),
            None
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 16, 0,))),
            None
        );
        assert_eq!(source.control(&plain(program_change(3, 79,))), None);
        assert_eq!(source.control(&plain(channel_pressure(3, 2,))), None);
        assert_eq!(source.control(&plain(timing_clock())), None);
        assert_eq!(source.control(&plain(start())), None);
        assert_eq!(source.control(&plain(r#continue())), None);
        assert_eq!(source.control(&plain(stop())), None);
        assert_eq!(source.control(&plain(active_sensing())), None);
        assert_eq!(source.control(&plain(system_reset())), None);
        assert_eq!(source.control(&pn(rpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(nrpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(rpn(1, 342, 45))), None);
        assert_eq!(source.control(&pn(nrpn(1, 520, 24))), None);
        assert_eq!(source.control(&plain(pitch_bend_change(6, 8192,))), None);
        assert_eq!(source.control(&tempo(120.0)), None);
        assert_eq!(source.feedback::<RawShortMessage>(fv(0.5)), None);
        assert_eq!(
            source.format_control_value(abs(0.5)).expect("bad").as_str(),
            "8192"
        );
    }

    #[test]
    fn control_change_14_bit_value_2() {
        // Given
        let source = TestMidiSource::ControlChange14BitValue {
            channel: Some(ch(1)),
            msb_controller_number: Some(cn(7)),
            custom_character: Default::default(),
        };
        // When
        // Then
        assert_eq!(source.control(&plain(control_change(3, 64, 127,))), None);
        assert_eq!(source.control(&plain(control_change(1, 64, 127,))), None);
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 4096))),
            None
        );
        assert_abs_diff_eq!(
            source
                .control(&cc(control_change_14_bit(1, 7, 16383)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source
                .control(&cc(control_change_14_bit(1, 7, 16383)))
                .unwrap(),
            frac(16383, 16383)
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.0)),
            Some(cc(control_change_14_bit(1, 7, 0)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.25)),
            Some(cc(control_change_14_bit(1, 7, 4096)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.5)),
            Some(cc(control_change_14_bit(1, 7, 8192)))
        );
        // In a center-oriented mapping this would yield 12288 instead of 12287
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.75)),
            Some(cc(control_change_14_bit(1, 7, 12287)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(1.0)),
            Some(cc(control_change_14_bit(1, 7, 16383)))
        );
    }

    #[test]
    fn parameter_number_value_1() {
        // Given
        let source = TestMidiSource::ParameterNumberValue {
            channel: None,
            number: None,
            is_14_bit: None,
            is_registered: None,
            custom_character: SourceCharacter::RangeElement,
        };
        // When
        // Then
        assert_eq!(source.control(&plain(note_on(0, 127, 55,))), None);
        assert_eq!(source.control(&plain(note_on(1, 0, 64,))), None);
        assert_eq!(source.control(&plain(note_off(0, 20, 100,))), None);
        assert_eq!(source.control(&plain(note_on(4, 20, 0,))), None);
        assert_eq!(source.control(&plain(control_change(3, 64, 127,))), None);
        assert_eq!(source.control(&plain(control_change(1, 64, 127,))), None);
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 4096))),
            None
        );
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 16383))),
            None
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 53, 127,))),
            None
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 16, 0,))),
            None
        );
        assert_eq!(source.control(&plain(program_change(3, 79,))), None);
        assert_eq!(source.control(&plain(channel_pressure(3, 2,))), None);
        assert_eq!(source.control(&plain(timing_clock())), None);
        assert_eq!(source.control(&plain(start())), None);
        assert_eq!(source.control(&plain(r#continue())), None);
        assert_eq!(source.control(&plain(stop())), None);
        assert_eq!(source.control(&plain(active_sensing())), None);
        assert_eq!(source.control(&plain(system_reset())), None);
        assert_abs_diff_eq!(
            source
                .control(&pn(rpn_14_bit(1, 520, 2048)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.12500762986022096)
        );
        assert_eq!(
            source.control(&pn(rpn_14_bit(1, 520, 2048))).unwrap(),
            frac(2048, 16383)
        );
        assert_abs_diff_eq!(
            source
                .control(&pn(nrpn_14_bit(1, 520, 16383)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source.control(&pn(nrpn_14_bit(1, 520, 16383))).unwrap(),
            frac(16383, 16383)
        );
        assert_abs_diff_eq!(
            source
                .control(&pn(rpn(1, 342, 0)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.0)
        );
        assert_eq!(source.control(&pn(rpn(1, 342, 0))).unwrap(), frac(0, 127));
        assert_abs_diff_eq!(
            source
                .control(&pn(nrpn(1, 520, 127)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source.control(&pn(nrpn(1, 520, 127))).unwrap(),
            frac(127, 127)
        );
        assert_eq!(source.control(&plain(pitch_bend_change(6, 8192,))), None);
        assert_eq!(source.control(&tempo(120.0)), None);
        assert_eq!(source.feedback::<RawShortMessage>(fv(0.5)), None);
        assert!(source.format_control_value(abs(0.5)).is_err());
    }

    #[test]
    fn parameter_number_value_2() {
        // Given
        let source = TestMidiSource::ParameterNumberValue {
            channel: Some(ch(7)),
            number: Some(u14(3000)),
            is_14_bit: Some(false),
            is_registered: Some(true),
            custom_character: SourceCharacter::RangeElement,
        };
        // When
        // Then
        assert_eq!(source.control(&plain(control_change(7, 64, 127,))), None);
        assert_eq!(source.control(&plain(control_change(7, 64, 127,))), None);
        assert_eq!(
            source.control(&cc(control_change_14_bit(7, 10, 4096))),
            None
        );
        assert_eq!(
            source.control(&cc(control_change_14_bit(7, 10, 16383))),
            None
        );
        assert_eq!(source.control(&pn(rpn_14_bit(7, 3000, 11253))), None);
        assert_abs_diff_eq!(
            source
                .control(&pn(rpn(7, 3000, 0)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(0.0)
        );
        assert_eq!(source.control(&pn(rpn(7, 3000, 0))).unwrap(), frac(0, 127));
        assert_eq!(source.control(&pn(nrpn_14_bit(7, 3000, 45))), None);
        assert_eq!(source.control(&pn(nrpn(7, 3000, 24))), None);
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.0)),
            Some(pn(rpn(7, 3000, 0)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.25)),
            Some(pn(rpn(7, 3000, 32)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.5)),
            Some(pn(rpn(7, 3000, 64)))
        );
        // In a center-oriented mapping this would yield 96 instead of 95
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.75)),
            Some(pn(rpn(7, 3000, 95)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(1.0)),
            Some(pn(rpn(7, 3000, 127)))
        );
        assert_eq!(
            source.format_control_value(abs(0.5)).expect("bad").as_str(),
            "64"
        );
    }

    #[test]
    fn parameter_number_value_2_toggle() {
        // Given
        let source = TestMidiSource::ParameterNumberValue {
            channel: Some(ch(7)),
            number: Some(u14(3000)),
            is_14_bit: Some(false),
            is_registered: Some(true),
            custom_character: SourceCharacter::ToggleButton,
        };
        // When
        // Then
        assert_abs_diff_eq!(
            source
                .control(&pn(rpn(7, 3000, 0)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source.control(&pn(rpn(7, 3000, 0))).unwrap(),
            frac(127, 127)
        );
        assert_abs_diff_eq!(
            source
                .control(&pn(rpn(7, 3000, 50)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source.control(&pn(rpn(7, 3000, 50))).unwrap(),
            frac(127, 127)
        );
        assert_abs_diff_eq!(
            source
                .control(&pn(rpn(7, 3000, 127)))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source.control(&pn(rpn(7, 3000, 127))).unwrap(),
            frac(127, 127)
        );
    }

    #[test]
    fn parameter_number_value_3() {
        // Given
        let source = TestMidiSource::ParameterNumberValue {
            channel: Some(ch(7)),
            number: Some(u14(3000)),
            is_14_bit: Some(true),
            is_registered: Some(true),
            custom_character: SourceCharacter::RangeElement,
        };
        // When
        // Then
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.0)),
            Some(pn(rpn_14_bit(7, 3000, 0)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.25)),
            Some(pn(rpn_14_bit(7, 3000, 4096)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.5)),
            Some(pn(rpn_14_bit(7, 3000, 8192)))
        );
        // In a center-oriented mapping this would yield 12288 instead of 12287
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(0.75)),
            Some(pn(rpn_14_bit(7, 3000, 12287)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(fv(1.0)),
            Some(pn(rpn_14_bit(7, 3000, 16383)))
        );
        assert_eq!(
            source.format_control_value(abs(0.5)).expect("bad").as_str(),
            "8192"
        );
    }

    #[test]
    fn clock_tempo() {
        // Given
        let source = TestMidiSource::ClockTempo;
        // When
        // Then
        assert_eq!(source.control(&plain(note_on(0, 127, 55,))), None);
        assert_eq!(source.control(&plain(note_on(1, 0, 64,))), None);
        assert_eq!(source.control(&plain(note_off(0, 20, 100,))), None);
        assert_eq!(source.control(&plain(note_on(4, 20, 0,))), None);
        assert_eq!(source.control(&plain(control_change(3, 64, 127,))), None);
        assert_eq!(source.control(&plain(control_change(1, 64, 127,))), None);
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 4096))),
            None
        );
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 16383))),
            None
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 53, 127,))),
            None
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 16, 0,))),
            None
        );
        assert_eq!(source.control(&plain(program_change(3, 79,))), None);
        assert_eq!(source.control(&plain(channel_pressure(3, 2,))), None);
        assert_eq!(source.control(&plain(timing_clock())), None);
        assert_eq!(source.control(&plain(start())), None);
        assert_eq!(source.control(&plain(r#continue())), None);
        assert_eq!(source.control(&plain(stop())), None);
        assert_eq!(source.control(&plain(active_sensing())), None);
        assert_eq!(source.control(&plain(system_reset())), None);
        assert_eq!(source.control(&pn(rpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(nrpn_14_bit(1, 520, 16383))), None);
        assert_eq!(source.control(&pn(rpn(1, 342, 45))), None);
        assert_eq!(source.control(&pn(nrpn(1, 520, 24))), None);
        assert_eq!(source.control(&plain(pitch_bend_change(6, 8192,))), None);
        assert_abs_diff_eq!(
            source.control(&tempo(120.0)).unwrap(),
            abs(0.12408759124087591)
        );
        assert_eq!(source.feedback::<RawShortMessage>(fv(0.5)), None);
        assert_eq!(
            source.format_control_value(abs(0.5)).expect("bad").as_str(),
            "480.50"
        );
    }

    #[test]
    fn clock_transport() {
        // Given
        let source = TestMidiSource::ClockTransport {
            message: MidiClockTransportMessage::Continue,
        };
        // When
        // Then
        assert_eq!(source.control(&plain(note_on(0, 127, 55,))), None);
        assert_eq!(source.control(&plain(note_on(1, 0, 64,))), None);
        assert_eq!(source.control(&plain(note_off(0, 20, 100,))), None);
        assert_eq!(source.control(&plain(note_on(4, 20, 0,))), None);
        assert_eq!(source.control(&plain(control_change(3, 64, 127,))), None);
        assert_eq!(source.control(&plain(control_change(1, 64, 127,))), None);
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 4096))),
            None
        );
        assert_eq!(
            source.control(&cc(control_change_14_bit(1, 10, 16383))),
            None
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 53, 127,))),
            None
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 16, 0,))),
            None
        );
        assert_eq!(source.control(&plain(program_change(3, 79,))), None);
        assert_eq!(source.control(&plain(channel_pressure(3, 2,))), None);
        assert_eq!(source.control(&plain(timing_clock())), None);
        assert_eq!(source.control(&plain(start())), None);
        assert_eq!(
            source
                .control(&plain(r#continue()))
                .unwrap()
                .to_absolute_continuous()
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(source.control(&plain(r#continue())).unwrap(), frac(1, 1));
        assert_eq!(source.control(&plain(stop())), None);
        assert_eq!(source.control(&plain(active_sensing())), None);
        assert_eq!(source.control(&plain(system_reset())), None);
        assert_eq!(source.control(&pn(rpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(nrpn_14_bit(1, 520, 16383))), None);
        assert_eq!(source.control(&pn(rpn(1, 342, 45))), None);
        assert_eq!(source.control(&pn(nrpn(1, 520, 24))), None);
        assert_eq!(source.control(&plain(pitch_bend_change(6, 8192,))), None);
        assert_eq!(source.control(&tempo(120.0)), None);
        assert_eq!(source.feedback::<RawShortMessage>(fv(0.5)), None);
        assert!(source.format_control_value(abs(0.5)).is_err());
    }

    fn abs(value: f64) -> ControlValue {
        ControlValue::absolute_continuous(value)
    }

    fn frac(actual: u32, max: u32) -> ControlValue {
        ControlValue::absolute_discrete(actual, max)
    }

    fn rel(increment: i32) -> ControlValue {
        ControlValue::relative(increment)
    }

    fn plain(msg: RawShortMessage) -> MidiSourceValue<'static, RawShortMessage> {
        MidiSourceValue::Plain(msg)
    }

    fn pn(msg: ParameterNumberMessage) -> MidiSourceValue<'static, RawShortMessage> {
        MidiSourceValue::ParameterNumber(msg)
    }

    fn cc(msg: ControlChange14BitMessage) -> MidiSourceValue<'static, RawShortMessage> {
        MidiSourceValue::ControlChange14Bit(msg)
    }

    fn fv(value: f64) -> FeedbackValue<'static> {
        FeedbackValue::Numeric(AbsoluteValue::Continuous(UnitValue::new(value)))
    }

    fn tempo(bpm: f64) -> MidiSourceValue<'static, RawShortMessage> {
        MidiSourceValue::Tempo(Bpm::new(bpm))
    }
}
