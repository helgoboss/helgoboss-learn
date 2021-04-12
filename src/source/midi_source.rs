use crate::{
    Bpm, ControlValue, DetailedSourceCharacter, DiscreteIncrement, MidiSourceScript,
    MidiSourceValue, RawMidiEvent, RawMidiPattern, UnitValue,
};
use derivative::Derivative;
use derive_more::Display;
use enum_iterator::IntoEnumIterator;
use num_enum::{IntoPrimitive, TryFromPrimitive};

use helgoboss_midi::{
    Channel, ControlChange14BitMessage, ControllerNumber, KeyNumber, ParameterNumberMessage,
    ShortMessage, ShortMessageFactory, ShortMessageType, StructuredShortMessage, U14, U7,
};
#[cfg(feature = "serde_repr")]
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::convert::TryFrom;

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
#[derivative(Eq, PartialEq, Hash)]
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
        #[derivative(PartialEq = "ignore", Hash = "ignore")]
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
        #[derivative(PartialEq = "ignore", Hash = "ignore")]
        custom_character: SourceCharacter,
    },
    // ParameterNumberMessage
    ParameterNumberValue {
        channel: Option<Channel>,
        number: Option<U14>,
        is_14_bit: Option<bool>,
        is_registered: Option<bool>,
        #[derivative(PartialEq = "ignore", Hash = "ignore")]
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
        #[derivative(PartialEq = "ignore", Hash = "ignore")]
        custom_character: SourceCharacter,
    },
    // For advanced feedback (e.g. to drive hardware displays).
    Script {
        #[derivative(PartialEq = "ignore", Hash = "ignore")]
        script: Option<S>,
    },
}

impl<S: MidiSourceScript> MidiSource<S> {
    pub fn from_source_value(source_value: MidiSourceValue<impl ShortMessage>) -> Option<Self> {
        use MidiSourceValue::*;
        let source = match source_value {
            ParameterNumber(msg) => MidiSource::ParameterNumberValue {
                channel: Some(msg.channel()),
                number: Some(msg.number()),
                is_14_bit: Some(msg.is_14_bit()),
                is_registered: Some(msg.is_registered()),
                custom_character: SourceCharacter::RangeElement,
            },
            ControlChange14Bit(msg) => MidiSource::ControlChange14BitValue {
                channel: Some(msg.channel()),
                msb_controller_number: Some(msg.msb_controller_number()),
                custom_character: SourceCharacter::RangeElement,
            },
            Tempo(_) => MidiSource::ClockTempo,
            Plain(msg) => MidiSource::from_short_message(msg)?,
            Raw(msg) => MidiSource::from_raw(msg.bytes()),
        };
        Some(source)
    }

    fn from_raw(msg: &[u8]) -> Self {
        MidiSource::Raw {
            pattern: RawMidiPattern::fixed_from_slice(msg),
            custom_character: Default::default(),
        }
    }

    fn from_short_message(msg: impl ShortMessage) -> Option<Self> {
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
                custom_character: SourceCharacter::RangeElement,
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
            ClockTempo | ClockTransport { .. } | Raw { .. } | Script { .. } => None,
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
            }
            | ParameterNumberValue {
                custom_character, ..
            } => custom_character.possible_detailed_characters(),
            // Usually a range control but sometimes more like a button (e.g. see #316).
            ProgramChangeNumber { .. } | ChannelPressureAmount { .. } => vec![
                DetailedSourceCharacter::RangeControl,
                DetailedSourceCharacter::MomentaryOnOffButton,
                DetailedSourceCharacter::PressOnlyButton,
            ],
            // Usually a range control but could also be a velocity-sensitive button.
            PolyphonicKeyPressureAmount { .. } | PitchBendChangeValue { .. } => vec![
                DetailedSourceCharacter::RangeControl,
                DetailedSourceCharacter::MomentaryVelocitySensitiveButton,
                DetailedSourceCharacter::MomentaryOnOffButton,
                DetailedSourceCharacter::PressOnlyButton,
            ],
            // Special targets for which we can safely say it's a range.
            NoteKeyNumber { .. } | ClockTempo => vec![DetailedSourceCharacter::RangeControl],
            // No control supported.
            Script { .. } => vec![],
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
                        Some(abs(UnitValue::MIN))
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
                        calc_control_value_from_7_bit_cc(*custom_character, control_value).ok()
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
                    calc_control_value_from_14_bit_cc(*custom_character, msg.value()).ok()
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
                    if msg.is_14_bit() {
                        calc_control_value_from_14_bit_cc(*custom_character, msg.value()).ok()
                    } else {
                        let u7_value = U7::try_from(msg.value()).unwrap();
                        calc_control_value_from_7_bit_cc(*custom_character, u7_value).ok()
                    }
                }
                _ => None,
            },
            S::ClockTransport { message } => match value {
                Plain(msg) if msg.r#type() == (*message).into() => Some(abs(UnitValue::MAX)),
                _ => None,
            },
            S::ClockTempo => match value {
                Tempo(bpm) => Some(abs(bpm.to_unit_value())),
                _ => None,
            },
            // TODO-low Support control for raw/sys-ex. Not difficult because we have the pattern
            //  structure already.
            S::Raw { .. } => None,
            // Feedback-only forever.
            S::Script { .. } => None,
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
        feedback_value: UnitValue,
    ) -> Option<MidiSourceValue<M>> {
        use MidiSource::*;
        use MidiSourceValue as V;
        match self {
            NoteVelocity {
                channel: Some(ch),
                key_number: Some(kn),
            } => Some(V::Plain(M::note_on(
                *ch,
                *kn,
                denormalize_7_bit(feedback_value),
            ))),
            NoteKeyNumber { channel: Some(ch) } => Some(V::Plain(M::note_on(
                *ch,
                denormalize_7_bit(feedback_value),
                U7::MAX,
            ))),
            PolyphonicKeyPressureAmount {
                channel: Some(ch),
                key_number: Some(kn),
            } => Some(V::Plain(M::polyphonic_key_pressure(
                *ch,
                *kn,
                denormalize_7_bit(feedback_value),
            ))),
            ControlChangeValue {
                channel: Some(ch),
                controller_number: Some(cn),
                ..
            } => Some(V::Plain(M::control_change(
                *ch,
                *cn,
                denormalize_7_bit(feedback_value),
            ))),
            ProgramChangeNumber { channel: Some(ch) } => Some(V::Plain(M::program_change(
                *ch,
                denormalize_7_bit(feedback_value),
            ))),
            ChannelPressureAmount { channel: Some(ch) } => Some(V::Plain(M::channel_pressure(
                *ch,
                denormalize_7_bit(feedback_value),
            ))),
            PitchBendChangeValue { channel: Some(ch) } => Some(V::Plain(M::pitch_bend_change(
                *ch,
                denormalize_14_bit_centered(feedback_value),
            ))),
            ControlChange14BitValue {
                channel: Some(ch),
                msb_controller_number: Some(mcn),
                ..
            } => Some(V::ControlChange14Bit(ControlChange14BitMessage::new(
                *ch,
                *mcn,
                denormalize_14_bit(feedback_value),
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
                        denormalize_7_bit(feedback_value),
                    )
                } else if !*is_registered && *is_14_bit {
                    ParameterNumberMessage::non_registered_14_bit(
                        *ch,
                        *n,
                        denormalize_14_bit(feedback_value),
                    )
                } else if *is_registered && !*is_14_bit {
                    ParameterNumberMessage::registered_7_bit(
                        *ch,
                        *n,
                        denormalize_7_bit(feedback_value),
                    )
                } else if *is_registered && *is_14_bit {
                    ParameterNumberMessage::registered_14_bit(
                        *ch,
                        *n,
                        denormalize_14_bit(feedback_value),
                    )
                } else {
                    unreachable!()
                };
                Some(V::ParameterNumber(n))
            }
            Raw { pattern, .. } => {
                let mut array = [0; RawMidiEvent::MAX_LENGTH];
                let mut i = 0u32;
                for byte in pattern
                    .byte_iter(feedback_value)
                    .take(RawMidiEvent::MAX_LENGTH)
                {
                    array[i as usize] = byte;
                    i += 1;
                }
                let raw_midi_event = RawMidiEvent::new(0, i, array);
                Some(V::Raw(Box::new(raw_midi_event)))
            }
            Script { script } => {
                let script = script.as_ref()?;
                let raw_midi_event = script.execute(feedback_value).ok()?;
                Some(V::Raw(raw_midi_event))
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
                let bpm = Bpm::from_unit_value(value.as_absolute()?);
                format!("{:.2}", bpm.get())
            }
            ClockTransport { .. } => {
                return Err("clock transport sources have just one possible control value");
            }
            _ => self
                .convert_control_value_to_midi_value(value.as_absolute()?)?
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
    fn convert_control_value_to_midi_value(&self, value: UnitValue) -> Result<i32, &'static str> {
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
            Raw { pattern, .. } => value.to_discrete(pattern.max_discrete_value()) as _,
            ClockTempo | ClockTransport { .. } | Script { .. } => {
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
                UnitValue::try_from_discrete(value as u16, pattern.max_discrete_value())?
            }
            ClockTempo | ClockTransport { .. } | Script { .. } => {
                return Err("not supported");
            }
        };
        Ok(unit_value)
    }
}

fn matches<T: PartialEq + Eq>(actual_value: T, configured_value: Option<T>) -> bool {
    match configured_value {
        None => true,
        Some(v) => actual_value == v,
    }
}

fn calc_control_value_from_7_bit_cc(
    character: SourceCharacter,
    cc_control_value: U7,
) -> Result<ControlValue, &'static str> {
    use SourceCharacter::*;
    let result = match character {
        RangeElement | MomentaryButton => abs(normalize_7_bit(cc_control_value)),
        Encoder1 => rel(DiscreteIncrement::from_encoder_1_value(cc_control_value)?),
        Encoder2 => rel(DiscreteIncrement::from_encoder_2_value(cc_control_value)?),
        Encoder3 => rel(DiscreteIncrement::from_encoder_3_value(cc_control_value)?),
        ToggleButton => abs(UnitValue::MAX),
    };
    Ok(result)
}

fn calc_control_value_from_14_bit_cc(
    character: SourceCharacter,
    cc_control_value: U14,
) -> Result<ControlValue, &'static str> {
    use SourceCharacter::*;
    let result = match character {
        RangeElement | MomentaryButton => abs(normalize_14_bit(cc_control_value)),
        Encoder1 | Encoder2 | Encoder3 => {
            let value_7_bit = extract_low_7_bit_value_from_14_bit_value(cc_control_value);
            let increment = match character {
                Encoder1 => DiscreteIncrement::from_encoder_1_value(value_7_bit)?,
                Encoder2 => DiscreteIncrement::from_encoder_2_value(value_7_bit)?,
                Encoder3 => DiscreteIncrement::from_encoder_3_value(value_7_bit)?,
                _ => unreachable!("impossible"),
            };
            rel(increment)
        }
        ToggleButton => abs(UnitValue::MAX),
    };
    Ok(result)
}

fn normalize_7_bit<T: Into<u8>>(value: T) -> UnitValue {
    UnitValue::new(value.into() as f64 / U7::MAX.get() as f64)
}

fn normalize_14_bit(value: U14) -> UnitValue {
    UnitValue::new(value.get() as f64 / U14::MAX.get() as f64)
}

/// See denormalize_14_bit_centered for an explanation
fn normalize_14_bit_centered(value: U14) -> UnitValue {
    if value == U14::MAX {
        return UnitValue::MAX;
    }
    UnitValue::new(value.get() as f64 / (U14::MAX.get() + 1) as f64)
}

fn denormalize_7_bit<T: From<U7>>(value: UnitValue) -> T {
    unsafe { U7::new_unchecked((value.get() * U7::MAX.get() as f64).round() as u8) }.into()
}

fn denormalize_14_bit<T: From<U14>>(value: UnitValue) -> T {
    unsafe { U14::new_unchecked((value.get() * U14::MAX.get() as f64).round() as u16) }.into()
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
fn denormalize_14_bit_centered<T: From<U14>>(value: UnitValue) -> T {
    let spread = (value.get() * (U14::MAX.get() + 1) as f64).round() as u16;
    unsafe { U14::new_unchecked(spread.min(16383)) }.into()
}

fn abs(value: UnitValue) -> ControlValue {
    ControlValue::Absolute(value)
}

fn rel(increment: DiscreteIncrement) -> ControlValue {
    ControlValue::Relative(increment)
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
            source.control(&plain(note_on(0, 64, 127,))).unwrap(),
            abs(1.0)
        );
        assert_abs_diff_eq!(
            source.control(&plain(note_on(0, 20, 0,))).unwrap(),
            abs(0.0)
        );
        assert_abs_diff_eq!(
            source.control(&plain(note_off(0, 20, 100,))).unwrap(),
            abs(0.0)
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
        assert_eq!(source.feedback::<RawShortMessage>(uv(0.5)), None);
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
            source.control(&plain(note_on(4, 20, 0,))).unwrap(),
            abs(0.0)
        );
        assert_eq!(source.control(&plain(note_off(15, 20, 100,))), None);
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.5)),
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
            source.control(&plain(note_on(0, 127, 55,))).unwrap(),
            abs(1.0)
        );
        assert_abs_diff_eq!(
            source.control(&plain(note_on(1, 0, 64,))).unwrap(),
            abs(0.0)
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
        assert_eq!(source.feedback::<RawShortMessage>(uv(0.5)), None);
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
            source.control(&plain(note_on(1, 0, 64,))).unwrap(),
            abs(0.0)
        );
        assert_eq!(source.control(&plain(note_off(0, 20, 100,))), None);
        assert_eq!(source.control(&plain(note_on(4, 20, 0,))), None);
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.5)),
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
                .unwrap(),
            abs(1.0)
        );
        assert_abs_diff_eq!(
            source
                .control(&plain(polyphonic_key_pressure(1, 16, 0,)))
                .unwrap(),
            abs(0.0)
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
        assert_eq!(source.feedback::<RawShortMessage>(uv(0.5)), None);
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
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source.control(&plain(polyphonic_key_pressure(1, 16, 0,))),
            None
        );
        assert_eq!(source.control(&plain(channel_pressure(3, 79,))), None);
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.5)),
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
            source.control(&plain(control_change(1, 64, 127,))).unwrap(),
            abs(1.0)
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
        assert_eq!(source.feedback::<RawShortMessage>(uv(0.5)), None);
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
            source.feedback::<RawShortMessage>(uv(0.0)),
            Some(plain(control_change(1, 64, 0)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.25)),
            Some(plain(control_change(1, 64, 32)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.5)),
            Some(plain(control_change(1, 64, 64)))
        );
        // In a center-oriented mapping this would yield 96 instead of 95
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.75)),
            Some(plain(control_change(1, 64, 95)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(1.0)),
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
            source.control(&plain(program_change(5, 0,))).unwrap(),
            abs(0.0)
        );
        assert_abs_diff_eq!(
            source.control(&plain(program_change(6, 127,))).unwrap(),
            abs(1.0)
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
        assert_eq!(source.feedback::<RawShortMessage>(uv(0.5)), None);
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
            source.control(&plain(program_change(10, 0,))).unwrap(),
            abs(0.0)
        );
        assert_eq!(source.control(&plain(program_change(6, 127,))), None);
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.5)),
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
            source.control(&plain(channel_pressure(5, 0,))).unwrap(),
            abs(0.0)
        );
        assert_abs_diff_eq!(
            source.control(&plain(channel_pressure(6, 127,))).unwrap(),
            abs(1.0)
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
        assert_eq!(source.feedback::<RawShortMessage>(uv(0.5)), None);
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
            source.control(&plain(channel_pressure(15, 127,))).unwrap(),
            abs(1.0)
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
            source.feedback::<RawShortMessage>(uv(0.5)),
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
            source.control(&plain(pitch_bend_change(5, 0,))).unwrap(),
            abs(0.0)
        );
        assert_abs_diff_eq!(
            source.control(&plain(pitch_bend_change(6, 4096,))).unwrap(),
            abs(0.25)
        );
        assert_abs_diff_eq!(
            source.control(&plain(pitch_bend_change(6, 8192,))).unwrap(),
            abs(0.5)
        );
        assert_abs_diff_eq!(
            source
                .control(&plain(pitch_bend_change(6, 12288,)))
                .unwrap(),
            abs(0.75)
        );
        assert_abs_diff_eq!(
            source
                .control(&plain(pitch_bend_change(6, 16383,)))
                .unwrap(),
            abs(1.0)
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
        assert_eq!(source.feedback::<RawShortMessage>(uv(0.5)), None);
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
            source.control(&plain(pitch_bend_change(3, 0,))).unwrap(),
            abs(0.0)
        );
        assert_eq!(source.control(&plain(pitch_bend_change(6, 8192,))), None);
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.0)),
            Some(plain(pitch_bend_change(3, 0)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.25)),
            Some(plain(pitch_bend_change(3, 4096)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.5)),
            Some(plain(pitch_bend_change(3, 8192)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.75)),
            Some(plain(pitch_bend_change(3, 12288)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(1.0)),
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
                .unwrap(),
            abs(0.2500152597204419)
        );
        assert_abs_diff_eq!(
            source
                .control(&cc(control_change_14_bit(1, 10, 16383)))
                .unwrap(),
            abs(1.0)
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
        assert_eq!(source.feedback::<RawShortMessage>(uv(0.5)), None);
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
                .unwrap(),
            abs(1.0)
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.0)),
            Some(cc(control_change_14_bit(1, 7, 0)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.25)),
            Some(cc(control_change_14_bit(1, 7, 4096)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.5)),
            Some(cc(control_change_14_bit(1, 7, 8192)))
        );
        // In a center-oriented mapping this would yield 12288 instead of 12287
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.75)),
            Some(cc(control_change_14_bit(1, 7, 12287)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(1.0)),
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
            source.control(&pn(rpn_14_bit(1, 520, 2048))).unwrap(),
            abs(0.12500762986022096)
        );
        assert_abs_diff_eq!(
            source.control(&pn(nrpn_14_bit(1, 520, 16383))).unwrap(),
            abs(1.0)
        );
        assert_abs_diff_eq!(source.control(&pn(rpn(1, 342, 0))).unwrap(), abs(0.0));
        assert_abs_diff_eq!(source.control(&pn(nrpn(1, 520, 127))).unwrap(), abs(1.0));
        assert_eq!(source.control(&plain(pitch_bend_change(6, 8192,))), None);
        assert_eq!(source.control(&tempo(120.0)), None);
        assert_eq!(source.feedback::<RawShortMessage>(uv(0.5)), None);
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
        assert_abs_diff_eq!(source.control(&pn(rpn(7, 3000, 0))).unwrap(), abs(0.0));
        assert_eq!(source.control(&pn(nrpn_14_bit(7, 3000, 45))), None);
        assert_eq!(source.control(&pn(nrpn(7, 3000, 24))), None);
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.0)),
            Some(pn(rpn(7, 3000, 0)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.25)),
            Some(pn(rpn(7, 3000, 32)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.5)),
            Some(pn(rpn(7, 3000, 64)))
        );
        // In a center-oriented mapping this would yield 96 instead of 95
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.75)),
            Some(pn(rpn(7, 3000, 95)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(1.0)),
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
        assert_abs_diff_eq!(source.control(&pn(rpn(7, 3000, 0))).unwrap(), abs(1.0));
        assert_abs_diff_eq!(source.control(&pn(rpn(7, 3000, 50))).unwrap(), abs(1.0));
        assert_abs_diff_eq!(source.control(&pn(rpn(7, 3000, 127))).unwrap(), abs(1.0));
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
            source.feedback::<RawShortMessage>(uv(0.0)),
            Some(pn(rpn_14_bit(7, 3000, 0)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.25)),
            Some(pn(rpn_14_bit(7, 3000, 4096)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.5)),
            Some(pn(rpn_14_bit(7, 3000, 8192)))
        );
        // In a center-oriented mapping this would yield 12288 instead of 12287
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(0.75)),
            Some(pn(rpn_14_bit(7, 3000, 12287)))
        );
        assert_eq!(
            source.feedback::<RawShortMessage>(uv(1.0)),
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
        assert_eq!(source.feedback::<RawShortMessage>(uv(0.5)), None);
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
        assert_eq!(source.control(&plain(r#continue())), Some(abs(1.0)));
        assert_eq!(source.control(&plain(stop())), None);
        assert_eq!(source.control(&plain(active_sensing())), None);
        assert_eq!(source.control(&plain(system_reset())), None);
        assert_eq!(source.control(&pn(rpn_14_bit(1, 520, 11253))), None);
        assert_eq!(source.control(&pn(nrpn_14_bit(1, 520, 16383))), None);
        assert_eq!(source.control(&pn(rpn(1, 342, 45))), None);
        assert_eq!(source.control(&pn(nrpn(1, 520, 24))), None);
        assert_eq!(source.control(&plain(pitch_bend_change(6, 8192,))), None);
        assert_eq!(source.control(&tempo(120.0)), None);
        assert_eq!(source.feedback::<RawShortMessage>(uv(0.5)), None);
        assert!(source.format_control_value(abs(0.5)).is_err());
    }

    fn abs(value: f64) -> ControlValue {
        ControlValue::absolute(value)
    }

    fn rel(increment: i32) -> ControlValue {
        ControlValue::relative(increment)
    }

    fn plain(msg: RawShortMessage) -> MidiSourceValue<RawShortMessage> {
        MidiSourceValue::Plain(msg)
    }

    fn pn(msg: ParameterNumberMessage) -> MidiSourceValue<RawShortMessage> {
        MidiSourceValue::ParameterNumber(msg)
    }

    fn cc(msg: ControlChange14BitMessage) -> MidiSourceValue<RawShortMessage> {
        MidiSourceValue::ControlChange14Bit(msg)
    }

    fn uv(value: f64) -> UnitValue {
        UnitValue::new(value)
    }

    fn tempo(bpm: f64) -> MidiSourceValue<RawShortMessage> {
        MidiSourceValue::Tempo(Bpm::new(bpm))
    }
}

fn extract_low_7_bit_value_from_14_bit_value(value: U14) -> U7 {
    U7::new((value.get() & 0x7f) as u8)
}
