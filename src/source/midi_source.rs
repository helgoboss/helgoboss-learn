use crate::{ControlValue, DiscreteIncrement, MidiSourceValue, UnitValue};

use helgoboss_midi::{
    ctrl_number_could_be_part_of_parameter_number_msg, FourteenBitValue, Midi14BitCcMessage,
    MidiMessage, MidiMessageFactory, MidiMessageKind, MidiParameterNumberMessage, Nibble,
    SevenBitValue, StructuredMidiMessage, FOURTEEN_BIT_VALUE_MAX, SEVEN_BIT_VALUE_MAX,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceCharacter {
    Range,
    Switch,
    Encoder1,
    Encoder2,
    Encoder3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiClockTransportMessageKind {
    Start,
    Continue,
    Stop,
}

impl From<MidiClockTransportMessageKind> for MidiMessageKind {
    fn from(kind: MidiClockTransportMessageKind) -> Self {
        use MidiClockTransportMessageKind::*;
        match kind {
            Start => MidiMessageKind::Start,
            Continue => MidiMessageKind::Continue,
            Stop => MidiMessageKind::Stop,
        }
    }
}

#[derive(Clone, Debug)]
pub enum MidiSource {
    NoteVelocity {
        channel: Option<Nibble>,
        key_number: Option<SevenBitValue>,
    },
    NoteKeyNumber {
        channel: Option<Nibble>,
    },
    // MidiMessageKind::PolyphonicKeyPressure
    PolyphonicKeyPressureAmount {
        channel: Option<Nibble>,
        key_number: Option<SevenBitValue>,
    },
    // MidiMessageKind::ControlChange
    ControlChangeValue {
        channel: Option<Nibble>,
        controller_number: Option<SevenBitValue>,
        custom_character: SourceCharacter,
    },
    // MidiMessageKind::ProgramChange
    ProgramChangeNumber {
        channel: Option<Nibble>,
    },
    // MidiMessageKind::ChannelPressure
    ChannelPressureAmount {
        channel: Option<Nibble>,
    },
    // MidiMessageKind::PitchBendChange
    PitchBendChangeValue {
        channel: Option<Nibble>,
    },
    // Midi14BitCcMessage
    FourteenBitCcMessageValue {
        channel: Option<Nibble>,
        msb_controller_number: Option<SevenBitValue>,
    },
    // MidiParameterNumberMessage
    ParameterNumberMessageValue {
        channel: Option<Nibble>,
        number: Option<FourteenBitValue>,
        is_14_bit: bool,
        is_registered: bool,
    },
    // MidiMessageKind::TimingClock
    ClockTempo,
    // MidiMessageKind::{Start, Continue, Stop}
    ClockTransport {
        message_kind: MidiClockTransportMessageKind,
    },
}

impl MidiSource {
    /// Determines the appropriate control value from the given MIDI source value. If this source
    /// doesn't process values of that kind, it returns None.
    pub fn get_control_value<M: MidiMessage>(
        &self,
        value: &MidiSourceValue<M>,
    ) -> Option<ControlValue> {
        use MidiSource as S;
        use MidiSourceValue::*;
        use StructuredMidiMessage::*;
        match self {
            S::NoteVelocity {
                channel,
                key_number,
            } => match value {
                PlainMessage(msg) => match msg.to_structured() {
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
                PlainMessage(msg) => match msg.to_structured() {
                    NoteOn {
                        channel: ch,
                        key_number,
                        velocity,
                    } if velocity > 0 && matches(ch, *channel) => {
                        Some(abs(normalize_7_bit(key_number)))
                    }
                    _ => None,
                },
                _ => None,
            },
            S::PitchBendChangeValue { channel } => match value {
                PlainMessage(msg) => match msg.to_structured() {
                    PitchBendChange {
                        channel: ch,
                        pitch_bend_value,
                    } if matches(ch, *channel) => Some(abs(normalize_14_bit(pitch_bend_value))),
                    _ => None,
                },
                _ => None,
            },
            S::ChannelPressureAmount { channel } => match value {
                PlainMessage(msg) => match msg.to_structured() {
                    ChannelPressure {
                        channel: ch,
                        pressure_amount,
                    } if matches(ch, *channel) => Some(abs(normalize_7_bit(pressure_amount))),
                    _ => None,
                },
                _ => None,
            },
            S::ProgramChangeNumber { channel } => match value {
                PlainMessage(msg) => match msg.to_structured() {
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
                PlainMessage(msg) => match msg.to_structured() {
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
                PlainMessage(msg) => match msg.to_structured() {
                    ControlChange {
                        channel: ch,
                        controller_number: cn,
                        control_value,
                    } if matches(ch, *channel) && matches(cn, *controller_number) => {
                        calc_control_value_from_control_change(*custom_character, control_value)
                            .ok()
                    }
                    _ => None,
                },
                _ => None,
            },
            S::FourteenBitCcMessageValue {
                channel,
                msb_controller_number,
            } => match value {
                FourteenBitCcMessage(msg)
                    if matches(msg.get_channel(), *channel)
                        && matches(msg.get_msb_controller_number(), *msb_controller_number) =>
                {
                    Some(abs(normalize_14_bit(msg.get_value())))
                }
                _ => None,
            },
            S::ParameterNumberMessageValue {
                channel,
                number,
                is_14_bit,
                is_registered,
            } => match value {
                ParameterNumberMessage(msg)
                    if matches(msg.get_channel(), *channel)
                        && matches(msg.get_number(), *number)
                        && msg.is_14_bit() == *is_14_bit
                        && msg.is_registered() == *is_registered =>
                {
                    let unit_value = if msg.is_14_bit() {
                        normalize_14_bit(msg.get_value())
                    } else {
                        normalize_7_bit(msg.get_value() as u8)
                    };
                    Some(abs(unit_value))
                }
                _ => None,
            },
            S::ClockTransport { message_kind } => match value {
                PlainMessage(msg) if msg.get_kind() == (*message_kind).into() => {
                    Some(abs(UnitValue::MAX))
                }
                _ => None,
            },
            S::ClockTempo => match value {
                TempoMessage { bpm } => Some(abs(UnitValue::new((*bpm - 1.0) / 960.0))),
                _ => None,
            },
        }
    }

    /// Checks if this source consumes the given MIDI message. This is for sources whose events are
    /// composed of multiple MIDI messages, which is 14-bit CC and (N)RPN.
    pub fn consumes(&self, msg: &impl MidiMessage) -> bool {
        use MidiSource::*;
        use StructuredMidiMessage::*;
        match self {
            FourteenBitCcMessageValue {
                channel,
                msb_controller_number,
            } => match msg.to_structured() {
                ControlChange {
                    channel: ch,
                    controller_number,
                    ..
                } => {
                    matches(ch, *channel)
                        && (matches(controller_number, *msb_controller_number)
                            || matches(controller_number, msb_controller_number.map(|n| n + 32)))
                }
                _ => false,
            },
            ParameterNumberMessageValue { channel, .. } => match msg.to_structured() {
                ControlChange {
                    channel: ch,
                    controller_number,
                    ..
                } => {
                    matches(ch, *channel)
                        && ctrl_number_could_be_part_of_parameter_number_msg(controller_number)
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Returns an appropriate MIDI source value for the given feedback value if feedback is
    /// supported by this source.
    pub fn get_feedback_value<M: MidiMessage + MidiMessageFactory>(
        &self,
        feedback_value: UnitValue,
    ) -> Option<MidiSourceValue<M>> {
        use MidiSource::*;
        use MidiSourceValue::*;
        match self {
            NoteVelocity {
                channel: Some(ch),
                key_number: Some(kn),
            } => Some(PlainMessage(M::note_on(
                *ch,
                *kn,
                denormalize_7_bit(feedback_value),
            ))),
            NoteKeyNumber { channel: Some(ch) } => Some(PlainMessage(M::note_on(
                *ch,
                denormalize_7_bit(feedback_value),
                SEVEN_BIT_VALUE_MAX,
            ))),
            PolyphonicKeyPressureAmount {
                channel: Some(ch),
                key_number: Some(kn),
            } => Some(PlainMessage(M::polyphonic_key_pressure(
                *ch,
                *kn,
                denormalize_7_bit(feedback_value),
            ))),
            ControlChangeValue {
                channel: Some(ch),
                controller_number: Some(cn),
                ..
            } => Some(PlainMessage(M::control_change(
                *ch,
                *cn,
                denormalize_7_bit(feedback_value),
            ))),
            ProgramChangeNumber { channel: Some(ch) } => Some(PlainMessage(M::program_change(
                *ch,
                denormalize_7_bit(feedback_value),
            ))),
            ChannelPressureAmount { channel: Some(ch) } => Some(PlainMessage(M::channel_pressure(
                *ch,
                denormalize_7_bit(feedback_value),
            ))),
            PitchBendChangeValue { channel: Some(ch) } => Some(PlainMessage(M::pitch_bend_change(
                *ch,
                // TODO Add test!
                denormalize_14_bit_ceil(feedback_value),
            ))),
            FourteenBitCcMessageValue {
                channel: Some(ch),
                msb_controller_number: Some(mcn),
            } => Some(FourteenBitCcMessage(Midi14BitCcMessage::new(
                *ch,
                *mcn,
                denormalize_14_bit(feedback_value),
            ))),
            ParameterNumberMessageValue {
                channel: Some(ch),
                number: Some(n),
                is_14_bit,
                is_registered,
            } => Some(ParameterNumberMessage(if *is_registered {
                if *is_14_bit {
                    MidiParameterNumberMessage::registered_14_bit(
                        *ch,
                        *n,
                        denormalize_14_bit(feedback_value),
                    )
                } else {
                    MidiParameterNumberMessage::registered_7_bit(
                        *ch,
                        *n,
                        denormalize_7_bit(feedback_value),
                    )
                }
            } else {
                if *is_14_bit {
                    MidiParameterNumberMessage::non_registered_14_bit(
                        *ch,
                        *n,
                        denormalize_14_bit(feedback_value),
                    )
                } else {
                    MidiParameterNumberMessage::non_registered_7_bit(
                        *ch,
                        *n,
                        denormalize_7_bit(feedback_value),
                    )
                }
            })),
            _ => None,
        }
    }
}

fn matches<T: PartialEq + Eq>(actual_value: T, configured_value: Option<T>) -> bool {
    match configured_value {
        None => true,
        Some(v) => actual_value == v,
    }
}

fn calc_control_value_from_control_change(
    character: SourceCharacter,
    cc_control_value: SevenBitValue,
) -> Result<ControlValue, ()> {
    use SourceCharacter::*;
    let result = match character {
        Encoder1 => rel(DiscreteIncrement::from_encoder_1_value(cc_control_value)?),
        Encoder2 => rel(DiscreteIncrement::from_encoder_2_value(cc_control_value)?),
        Encoder3 => rel(DiscreteIncrement::from_encoder_3_value(cc_control_value)?),
        _ => abs(normalize_7_bit(cc_control_value)),
    };
    Ok(result)
}

fn normalize_7_bit(value: SevenBitValue) -> UnitValue {
    UnitValue::new(value as f64 / SEVEN_BIT_VALUE_MAX as f64)
}

fn normalize_14_bit(value: FourteenBitValue) -> UnitValue {
    UnitValue::new(value as f64 / FOURTEEN_BIT_VALUE_MAX as f64)
}

fn denormalize_7_bit(value: UnitValue) -> SevenBitValue {
    (value.get_number() * SEVEN_BIT_VALUE_MAX as f64).round() as SevenBitValue
}

fn denormalize_14_bit(value: UnitValue) -> FourteenBitValue {
    (value.get_number() * FOURTEEN_BIT_VALUE_MAX as f64).round() as FourteenBitValue
}

/// This uses `ceil()` instead of `round()`. Should be used for pitch bend because it's centered.
/// The center is not an integer (because there's an even number of possible values) and the
/// official center is considered as the next higher value.
///
/// - Example uncentered: Possible pitch bend values go from 0 to 16383. Exact center would be
///   8191.5. Official center is 8192.
/// - Example centered: Possible pitch bend values go from -8192 to 8191. Exact center would be
///   -0.5. Official center is 0.
fn denormalize_14_bit_ceil(value: UnitValue) -> FourteenBitValue {
    (value.get_number() * FOURTEEN_BIT_VALUE_MAX as f64).ceil() as FourteenBitValue
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
    use helgoboss_midi::{MidiMessageFactory, RawMidiMessage};
    use MidiSourceValue::*;

    #[test]
    fn default() {
        // Given
        let source = MidiSource::NoteVelocity {
            channel: None,
            key_number: None,
        };
        // When
        source.get_control_value(&PlainMessage(RawMidiMessage::note_on(0, 64, 100)));
    }
}
