use crate::{ControlValue, DiscreteIncrement, MidiSourceValue, UnitValue};

use helgoboss_midi::{
    Channel, ControllerNumber, KeyNumber, MidiControlChange14BitMessage, MidiMessage,
    MidiMessageFactory, MidiMessageType, MidiParameterNumberMessage, StructuredMidiMessage, U14,
    U7,
};
use std::convert::{TryFrom, TryInto};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceCharacter {
    Range,
    Switch,
    Encoder1,
    Encoder2,
    Encoder3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiClockTransportMessage {
    Start,
    Continue,
    Stop,
}

impl From<MidiClockTransportMessage> for MidiMessageType {
    fn from(msg: MidiClockTransportMessage) -> Self {
        use MidiClockTransportMessage::*;
        match msg {
            Start => MidiMessageType::Start,
            Continue => MidiMessageType::Continue,
            Stop => MidiMessageType::Stop,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MidiSource {
    NoteVelocity {
        channel: Option<Channel>,
        key_number: Option<KeyNumber>,
    },
    NoteKeyNumber {
        channel: Option<Channel>,
    },
    // MidiMessageType::PolyphonicKeyPressure
    PolyphonicKeyPressureAmount {
        channel: Option<Channel>,
        key_number: Option<KeyNumber>,
    },
    // MidiMessageType::ControlChange
    ControlChangeValue {
        channel: Option<Channel>,
        controller_number: Option<ControllerNumber>,
        custom_character: SourceCharacter,
    },
    // MidiMessageType::ProgramChange
    ProgramChangeNumber {
        channel: Option<Channel>,
    },
    // MidiMessageType::ChannelPressure
    ChannelPressureAmount {
        channel: Option<Channel>,
    },
    // MidiMessageType::PitchBendChange
    PitchBendChangeValue {
        channel: Option<Channel>,
    },
    // MidiControlChange14BitMessage
    ControlChange14BitValue {
        channel: Option<Channel>,
        msb_controller_number: Option<ControllerNumber>,
    },
    // MidiParameterNumberMessage
    ParameterNumberValue {
        channel: Option<Channel>,
        number: Option<U14>,
        is_14_bit: Option<bool>,
        is_registered: Option<bool>,
    },
    // MidiMessageType::TimingClock
    ClockTempo,
    // MidiMessageType::{Start, Continue, Stop}
    ClockTransport {
        message: MidiClockTransportMessage,
    },
}

impl MidiSource {
    /// Determines the appropriate control value from the given MIDI source value. If this source
    /// doesn't process values of that type, it returns None.
    pub fn control<M: MidiMessage>(&self, value: &MidiSourceValue<M>) -> Option<ControlValue> {
        use MidiSource as S;
        use MidiSourceValue::*;
        use StructuredMidiMessage::*;
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
                        calc_control_value_from_control_change(*custom_character, control_value)
                            .ok()
                    }
                    _ => None,
                },
                _ => None,
            },
            S::ControlChange14BitValue {
                channel,
                msb_controller_number,
            } => match value {
                ControlChange14Bit(msg)
                    if matches(msg.get_channel(), *channel)
                        && matches(msg.get_msb_controller_number(), *msb_controller_number) =>
                {
                    Some(abs(normalize_14_bit(msg.get_value())))
                }
                _ => None,
            },
            S::ParameterNumberValue {
                channel,
                number,
                is_14_bit,
                is_registered,
            } => match value {
                ParameterNumber(msg)
                    if matches(msg.get_channel(), *channel)
                        && matches(msg.get_number(), *number)
                        && matches(msg.is_14_bit(), *is_14_bit)
                        && matches(msg.is_registered(), *is_registered) =>
                {
                    let unit_value = if msg.is_14_bit() {
                        normalize_14_bit(msg.get_value())
                    } else {
                        normalize_7_bit(U7::try_from(msg.get_value()).unwrap())
                    };
                    Some(abs(unit_value))
                }
                _ => None,
            },
            S::ClockTransport { message } => match value {
                Plain(msg) if msg.get_type() == (*message).into() => Some(abs(UnitValue::MAX)),
                _ => None,
            },
            S::ClockTempo => match value {
                Tempo { bpm } => Some(abs(UnitValue::new((*bpm - 1.0) / 960.0))),
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
            ControlChange14BitValue {
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
                            || matches(
                                controller_number,
                                msb_controller_number
                                    .map(|n| n.get_corresponding_14_bit_lsb().unwrap()),
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
                        && controller_number.can_be_part_of_parameter_number_message()
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Returns an appropriate MIDI source value for the given feedback value if feedback is
    /// supported by this source.
    pub fn feedback<M: MidiMessage + MidiMessageFactory>(
        &self,
        feedback_value: UnitValue,
    ) -> Option<MidiSourceValue<M>> {
        use MidiSource::*;
        use MidiSourceValue::*;
        match self {
            NoteVelocity {
                channel: Some(ch),
                key_number: Some(kn),
            } => Some(Plain(M::note_on(
                *ch,
                *kn,
                denormalize_7_bit(feedback_value),
            ))),
            NoteKeyNumber { channel: Some(ch) } => Some(Plain(M::note_on(
                *ch,
                denormalize_7_bit(feedback_value),
                U7::MAX,
            ))),
            PolyphonicKeyPressureAmount {
                channel: Some(ch),
                key_number: Some(kn),
            } => Some(Plain(M::polyphonic_key_pressure(
                *ch,
                *kn,
                denormalize_7_bit(feedback_value),
            ))),
            ControlChangeValue {
                channel: Some(ch),
                controller_number: Some(cn),
                ..
            } => Some(Plain(M::control_change(
                *ch,
                *cn,
                denormalize_7_bit(feedback_value),
            ))),
            ProgramChangeNumber { channel: Some(ch) } => Some(Plain(M::program_change(
                *ch,
                denormalize_7_bit(feedback_value),
            ))),
            ChannelPressureAmount { channel: Some(ch) } => Some(Plain(M::channel_pressure(
                *ch,
                denormalize_7_bit(feedback_value),
            ))),
            PitchBendChangeValue { channel: Some(ch) } => Some(Plain(M::pitch_bend_change(
                *ch,
                denormalize_14_bit_centered(feedback_value),
            ))),
            ControlChange14BitValue {
                channel: Some(ch),
                msb_controller_number: Some(mcn),
            } => Some(ControlChange14Bit(MidiControlChange14BitMessage::new(
                *ch,
                *mcn,
                denormalize_14_bit(feedback_value),
            ))),
            ParameterNumberValue {
                channel: Some(ch),
                number: Some(n),
                is_14_bit: Some(is_14_bit),
                is_registered: Some(is_registered),
            } => Some(ParameterNumber(if *is_registered {
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
    cc_control_value: U7,
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

fn normalize_7_bit<T: Into<u8>>(value: T) -> UnitValue {
    UnitValue::new(value.into() as f64 / u8::from(U7::MAX) as f64)
}

fn normalize_14_bit(value: U14) -> UnitValue {
    UnitValue::new(u16::from(value) as f64 / u16::from(U14::MAX) as f64)
}

/// See denormalize_14_bit_centered for an explanation
fn normalize_14_bit_centered(value: U14) -> UnitValue {
    if value == U14::MAX {
        return UnitValue::MAX;
    }
    UnitValue::new(u16::from(value) as f64 / U14::COUNT as f64)
}

fn denormalize_7_bit<T: From<U7>>(value: UnitValue) -> T {
    unsafe { U7::new_unchecked((value.get() * u8::from(U7::MAX) as f64).round() as u8) }.into()
}

fn denormalize_14_bit(value: UnitValue) -> U14 {
    unsafe { U14::new_unchecked((value.get() * u16::from(U14::MAX) as f64).round() as u16) }
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
fn denormalize_14_bit_centered(value: UnitValue) -> U14 {
    let spread = (value.get() * U14::COUNT as f64).round() as u16;
    unsafe { U14::new_unchecked(spread.min(16383)) }
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
    use approx::*;
    use helgoboss_midi::test_util::{channel as ch, controller_number as cn, key_number as kn, *};
    use helgoboss_midi::RawMidiMessage;

    #[test]
    fn note_velocity_1() {
        // Given
        let source = MidiSource::NoteVelocity {
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
        assert_eq!(source.feedback::<RawMidiMessage>(uv(0.5)), None);
    }

    #[test]
    fn note_velocity_2() {
        // Given
        let source = MidiSource::NoteVelocity {
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
            source.feedback::<RawMidiMessage>(uv(0.5)),
            Some(plain(note_on(4, 20, 64)))
        );
    }

    #[test]
    fn note_key_number_1() {
        // Given
        let source = MidiSource::NoteKeyNumber { channel: None };
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
        assert_eq!(source.feedback::<RawMidiMessage>(uv(0.5)), None);
    }

    #[test]
    fn note_key_number_2() {
        // Given
        let source = MidiSource::NoteKeyNumber {
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
            source.feedback::<RawMidiMessage>(uv(0.5)),
            Some(plain(note_on(1, 64, 127)))
        );
    }

    #[test]
    fn polyphonic_key_pressure_amount_1() {
        // Given
        let source = MidiSource::PolyphonicKeyPressureAmount {
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
        assert_eq!(source.feedback::<RawMidiMessage>(uv(0.5)), None);
    }

    #[test]
    fn polyphonic_key_pressure_amount_2() {
        // Given
        let source = MidiSource::PolyphonicKeyPressureAmount {
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
            source.feedback::<RawMidiMessage>(uv(0.5)),
            Some(plain(polyphonic_key_pressure(1, 53, 64)))
        );
    }

    #[test]
    fn control_change_value_1() {
        // Given
        let source = MidiSource::ControlChangeValue {
            channel: Some(ch(1)),
            controller_number: None,
            custom_character: SourceCharacter::Range,
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
        assert_eq!(source.feedback::<RawMidiMessage>(uv(0.5)), None);
    }

    #[test]
    fn control_change_value_2() {
        // Given
        let source = MidiSource::ControlChangeValue {
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
            source.feedback::<RawMidiMessage>(uv(0.0)),
            Some(plain(control_change(1, 64, 0)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.25)),
            Some(plain(control_change(1, 64, 32)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.5)),
            Some(plain(control_change(1, 64, 64)))
        );
        // In a center-oriented mapping this would yield 96 instead of 95
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.75)),
            Some(plain(control_change(1, 64, 95)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(1.0)),
            Some(plain(control_change(1, 64, 127)))
        );
    }

    #[test]
    fn program_change_number_1() {
        // Given
        let source = MidiSource::ProgramChangeNumber { channel: None };
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
        assert_eq!(source.feedback::<RawMidiMessage>(uv(0.5)), None);
    }

    #[test]
    fn program_change_number_2() {
        // Given
        let source = MidiSource::ProgramChangeNumber {
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
            source.feedback::<RawMidiMessage>(uv(0.5)),
            Some(plain(program_change(10, 64)))
        );
    }

    #[test]
    fn channel_pressure_amount_1() {
        // Given
        let source = MidiSource::ChannelPressureAmount { channel: None };
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
        assert_eq!(source.feedback::<RawMidiMessage>(uv(0.5)), None);
    }

    #[test]
    fn channel_pressure_amount_2() {
        // Given
        let source = MidiSource::ChannelPressureAmount {
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
            source.feedback::<RawMidiMessage>(uv(0.5)),
            Some(plain(channel_pressure(15, 64)))
        );
    }

    #[test]
    fn pitch_bend_change_value_1() {
        // Given
        let source = MidiSource::PitchBendChangeValue { channel: None };
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
        assert_eq!(source.feedback::<RawMidiMessage>(uv(0.5)), None);
    }

    #[test]
    fn pitch_bend_change_value_2() {
        // Given
        let source = MidiSource::PitchBendChangeValue {
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
            source.feedback::<RawMidiMessage>(uv(0.0)),
            Some(plain(pitch_bend_change(3, 0)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.25)),
            Some(plain(pitch_bend_change(3, 4096)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.5)),
            Some(plain(pitch_bend_change(3, 8192)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.75)),
            Some(plain(pitch_bend_change(3, 12288)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(1.0)),
            Some(plain(pitch_bend_change(3, 16383)))
        );
    }

    #[test]
    fn control_change_14_bit_value_1() {
        // Given
        let source = MidiSource::ControlChange14BitValue {
            channel: Some(ch(1)),
            msb_controller_number: None,
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
        assert_eq!(source.feedback::<RawMidiMessage>(uv(0.5)), None);
    }

    #[test]
    fn control_change_14_bit_value_2() {
        // Given
        let source = MidiSource::ControlChange14BitValue {
            channel: Some(ch(1)),
            msb_controller_number: Some(cn(7)),
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
            source.feedback::<RawMidiMessage>(uv(0.0)),
            Some(cc(control_change_14_bit(1, 7, 0)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.25)),
            Some(cc(control_change_14_bit(1, 7, 4096)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.5)),
            Some(cc(control_change_14_bit(1, 7, 8192)))
        );
        // In a center-oriented mapping this would yield 12288 instead of 12287
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.75)),
            Some(cc(control_change_14_bit(1, 7, 12287)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(1.0)),
            Some(cc(control_change_14_bit(1, 7, 16383)))
        );
    }

    #[test]
    fn parameter_number_value_1() {
        // Given
        let source = MidiSource::ParameterNumberValue {
            channel: None,
            number: None,
            is_14_bit: None,
            is_registered: None,
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
        assert_eq!(source.feedback::<RawMidiMessage>(uv(0.5)), None);
    }

    #[test]
    fn parameter_number_value_2() {
        // Given
        let source = MidiSource::ParameterNumberValue {
            channel: Some(ch(7)),
            number: Some(u14(3000)),
            is_14_bit: Some(false),
            is_registered: Some(true),
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
            source.feedback::<RawMidiMessage>(uv(0.0)),
            Some(pn(rpn(7, 3000, 0)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.25)),
            Some(pn(rpn(7, 3000, 32)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.5)),
            Some(pn(rpn(7, 3000, 64)))
        );
        // In a center-oriented mapping this would yield 96 instead of 95
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.75)),
            Some(pn(rpn(7, 3000, 95)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(1.0)),
            Some(pn(rpn(7, 3000, 127)))
        );
    }

    #[test]
    fn parameter_number_value_3() {
        // Given
        let source = MidiSource::ParameterNumberValue {
            channel: Some(ch(7)),
            number: Some(u14(3000)),
            is_14_bit: Some(true),
            is_registered: Some(true),
        };
        // When
        // Then
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.0)),
            Some(pn(rpn_14_bit(7, 3000, 0)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.25)),
            Some(pn(rpn_14_bit(7, 3000, 4096)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.5)),
            Some(pn(rpn_14_bit(7, 3000, 8192)))
        );
        // In a center-oriented mapping this would yield 12288 instead of 12287
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(0.75)),
            Some(pn(rpn_14_bit(7, 3000, 12287)))
        );
        assert_eq!(
            source.feedback::<RawMidiMessage>(uv(1.0)),
            Some(pn(rpn_14_bit(7, 3000, 16383)))
        );
    }

    #[test]
    fn clock_tempo() {
        // Given
        let source = MidiSource::ClockTempo;
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
            abs(0.12395833333333334)
        );
        assert_eq!(source.feedback::<RawMidiMessage>(uv(0.5)), None);
    }

    #[test]
    fn clock_transport() {
        // Given
        let source = MidiSource::ClockTransport {
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
        assert_eq!(source.feedback::<RawMidiMessage>(uv(0.5)), None);
    }

    fn abs(value: f64) -> ControlValue {
        ControlValue::absolute(value)
    }

    fn rel(increment: i32) -> ControlValue {
        ControlValue::relative(increment)
    }

    fn plain(msg: RawMidiMessage) -> MidiSourceValue<RawMidiMessage> {
        MidiSourceValue::Plain(msg)
    }

    fn pn(msg: MidiParameterNumberMessage) -> MidiSourceValue<RawMidiMessage> {
        MidiSourceValue::ParameterNumber(msg)
    }

    fn cc(msg: MidiControlChange14BitMessage) -> MidiSourceValue<RawMidiMessage> {
        MidiSourceValue::ControlChange14Bit(msg)
    }

    fn uv(value: f64) -> UnitValue {
        UnitValue::new(value)
    }

    fn tempo(bpm: f64) -> MidiSourceValue<RawMidiMessage> {
        MidiSourceValue::Tempo { bpm }
    }
}
