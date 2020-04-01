use crate::{ControlValue, DiscreteIncrement, MidiSourceValue, UnitValue};

use helgoboss_midi::{
    data_could_be_part_of_parameter_number_msg, FourteenBitValue, MidiMessage, MidiMessageKind,
    Nibble, SevenBitValue, StructuredMidiMessage, FOURTEEN_BIT_VALUE_MAX, SEVEN_BIT_VALUE_MAX,
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
    // TODO Check if these kind of "anonymous inline" enum structs are really enough
    // MidiMessageKind::{NoteOff, NoteOn}
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
    /// Usually called very early right in the audio thread in order to determine if it's at all
    /// necessary to process the source value and to determine if the value should be let through
    /// or not.
    pub fn processes<M: MidiMessage>(&self, value: &MidiSourceValue<M>) -> bool {
        use MidiSource::*;
        use MidiSourceValue::*;
        match self {
            NoteVelocity {
                channel,
                key_number,
            } => match value {
                PlainMessage(msg) => {
                    msg.is_note()
                        && matches(&msg.get_channel().unwrap(), channel)
                        && matches(&msg.get_data_byte_1(), key_number)
                }
                _ => false,
            },
            NoteKeyNumber { channel } => match value {
                PlainMessage(msg) => {
                    msg.is_note_on() && matches(&msg.get_channel().unwrap(), channel)
                }
                _ => false,
            },
            PitchBendChangeValue { channel } => match value {
                PlainMessage(msg) => {
                    msg.get_kind() == MidiMessageKind::PitchBendChange
                        && matches(&msg.get_channel().unwrap(), channel)
                }
                _ => false,
            },
            ChannelPressureAmount { channel } => match value {
                PlainMessage(msg) => {
                    msg.get_kind() == MidiMessageKind::ChannelPressure
                        && matches(&msg.get_channel().unwrap(), channel)
                }
                _ => false,
            },
            ProgramChangeNumber { channel } => match value {
                PlainMessage(msg) => {
                    msg.get_kind() == MidiMessageKind::ProgramChange
                        && matches(&msg.get_channel().unwrap(), channel)
                }
                _ => false,
            },
            PolyphonicKeyPressureAmount {
                channel,
                key_number,
            } => match value {
                PlainMessage(msg) => {
                    msg.get_kind() == MidiMessageKind::PolyphonicKeyPressure
                        && matches(&msg.get_channel().unwrap(), channel)
                        && matches(&msg.get_data_byte_1(), key_number)
                }
                _ => false,
            },
            ControlChangeValue {
                channel,
                controller_number,
                custom_character,
            } => match value {
                PlainMessage(msg) => match msg.to_structured() {
                    StructuredMidiMessage::ControlChange(data) => {
                        matches(&data.channel, channel)
                            && matches(&data.controller_number, controller_number)
                            && calc_control_value_from_control_change(
                                *custom_character,
                                data.control_value,
                            )
                            .is_ok()
                    }
                    _ => false,
                },
                _ => false,
            },
            FourteenBitCcMessageValue {
                channel,
                msb_controller_number,
            } => match value {
                FourteenBitCcMessage(msg) => {
                    matches(&msg.get_channel(), channel)
                        && matches(&msg.get_msb_controller_number(), msb_controller_number)
                }
                _ => false,
            },
            ParameterNumberMessageValue {
                channel,
                number,
                is_14_bit,
                is_registered,
            } => match value {
                ParameterNumberMessage(msg) => {
                    matches(&msg.get_channel(), channel)
                        && matches(&msg.get_number(), number)
                        && msg.is_14_bit() == *is_14_bit
                        && msg.is_registered() == *is_registered
                }
                _ => false,
            },
            ClockTransport { message_kind } => match value {
                PlainMessage(msg) => msg.get_kind() == (*message_kind).into(),
                _ => false,
            },
            ClockTempo => match value {
                TempoMessage { .. } => true,
                _ => false,
            },
        }
    }

    /// Returns Err if this source can't process the given source value type. However, this doesn't
    /// do a complete check if this value should be processed. Please see
    /// [processes_source_value](#method.processes_source_value). This has been split because
    /// it's quite likely that those methods must be called from different threads.
    pub fn get_control_value<M: MidiMessage>(
        &self,
        value: &MidiSourceValue<M>,
    ) -> Result<ControlValue, ()> {
        use MidiSource::*;
        use MidiSourceValue::*;
        match self {
            NoteVelocity { .. } => match value {
                PlainMessage(msg) => match msg.to_structured() {
                    StructuredMidiMessage::NoteOn(data) => Ok(ControlValue::absolute(
                        data.velocity as f64 / SEVEN_BIT_VALUE_MAX as f64,
                    )),
                    StructuredMidiMessage::NoteOff(_) => Ok(ControlValue::absolute(0.0)),
                    _ => Err(()),
                },
                _ => Err(()),
            },
            NoteKeyNumber { .. } => match value {
                PlainMessage(msg) => match msg.to_structured() {
                    StructuredMidiMessage::NoteOn(data) => Ok(ControlValue::absolute(
                        data.key_number as f64 / SEVEN_BIT_VALUE_MAX as f64,
                    )),
                    _ => Err(()),
                },
                _ => Err(()),
            },
            PitchBendChangeValue { .. } => match value {
                PlainMessage(msg) => match msg.to_structured() {
                    StructuredMidiMessage::PitchBendChange(data) => Ok(ControlValue::absolute(
                        data.pitch_bend_value as f64 / FOURTEEN_BIT_VALUE_MAX as f64,
                    )),
                    _ => Err(()),
                },
                _ => Err(()),
            },
            ChannelPressureAmount { .. } => match value {
                PlainMessage(msg) => match msg.to_structured() {
                    StructuredMidiMessage::ChannelPressure(data) => Ok(ControlValue::absolute(
                        data.pressure_amount as f64 / SEVEN_BIT_VALUE_MAX as f64,
                    )),
                    _ => Err(()),
                },
                _ => Err(()),
            },
            ProgramChangeNumber { .. } => match value {
                PlainMessage(msg) => match msg.to_structured() {
                    StructuredMidiMessage::ProgramChange(data) => Ok(ControlValue::absolute(
                        data.program_number as f64 / SEVEN_BIT_VALUE_MAX as f64,
                    )),
                    _ => Err(()),
                },
                _ => Err(()),
            },
            PolyphonicKeyPressureAmount { .. } => match value {
                PlainMessage(msg) => match msg.to_structured() {
                    StructuredMidiMessage::PolyphonicKeyPressure(data) => {
                        Ok(ControlValue::absolute(
                            data.pressure_amount as f64 / SEVEN_BIT_VALUE_MAX as f64,
                        ))
                    }
                    _ => Err(()),
                },
                _ => Err(()),
            },
            ControlChangeValue {
                custom_character, ..
            } => match value {
                PlainMessage(msg) => match msg.to_structured() {
                    StructuredMidiMessage::ControlChange(data) => {
                        Ok(calc_control_value_from_control_change(
                            *custom_character,
                            data.control_value,
                        )?)
                    }
                    _ => Err(()),
                },
                _ => Err(()),
            },
            FourteenBitCcMessageValue { .. } => match value {
                FourteenBitCcMessage(msg) => Ok(ControlValue::absolute(
                    msg.get_value() as f64 / FOURTEEN_BIT_VALUE_MAX as f64,
                )),
                _ => Err(()),
            },
            ParameterNumberMessageValue { .. } => match value {
                ParameterNumberMessage(msg) => Ok(ControlValue::absolute(
                    msg.get_value() as f64
                        / if msg.is_14_bit() {
                            FOURTEEN_BIT_VALUE_MAX as f64
                        } else {
                            SEVEN_BIT_VALUE_MAX as f64
                        },
                )),
                _ => Err(()),
            },
            ClockTransport { .. } => Ok(ControlValue::absolute(1.0)),
            ClockTempo => match value {
                TempoMessage { bpm } => Ok(ControlValue::absolute((*bpm - 1.0) / 960.0)),
                _ => Err(()),
            },
        }
    }

    // Only has to be implemented for sources whose events are composed of multiple MIDI messages
    pub fn consumes(&self, msg: &impl MidiMessage) -> bool {
        use MidiSource::*;
        match self {
            FourteenBitCcMessageValue {
                channel,
                msb_controller_number,
            } => match msg.to_structured() {
                StructuredMidiMessage::ControlChange(data) => {
                    matches(&data.channel, channel)
                        && (matches(&data.controller_number, msb_controller_number)
                            || matches(
                                &data.controller_number,
                                &msb_controller_number.map(|n| n + 32),
                            ))
                }
                _ => false,
            },
            ParameterNumberMessageValue { channel, .. } => match msg.to_structured() {
                StructuredMidiMessage::ControlChange(data) => {
                    matches(&data.channel, channel)
                        && data_could_be_part_of_parameter_number_msg(&data)
                }
                _ => false,
            },
            _ => false,
        }
    }
}

fn matches<T: PartialEq>(actual_value: &T, configured_value: &Option<T>) -> bool {
    match configured_value {
        None => true,
        Some(v) => actual_value == v,
    }
}

fn calc_control_value_from_control_change(
    character: SourceCharacter,
    cc_control_value: SevenBitValue,
) -> Result<ControlValue, ()> {
    use ControlValue::*;
    use SourceCharacter::*;
    let result = match character {
        Encoder1 => Relative(DiscreteIncrement::from_encoder_1_value(cc_control_value)?),
        Encoder2 => Relative(DiscreteIncrement::from_encoder_2_value(cc_control_value)?),
        Encoder3 => Relative(DiscreteIncrement::from_encoder_2_value(cc_control_value)?),
        _ => Absolute(UnitValue::new(
            cc_control_value as f64 / SEVEN_BIT_VALUE_MAX as f64,
        )),
    };
    Ok(result)
}
