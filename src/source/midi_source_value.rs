use crate::{DiscreteValue, Interval, UnitValue};
use derive_more::Display;
use helgoboss_midi::{
    ControlChange14BitMessage, ParameterNumberMessage, ShortMessage, ShortMessageFactory,
};

/// Incoming value which might be used to control something
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum MidiSourceValue<M: ShortMessage> {
    Plain(M),
    ParameterNumber(ParameterNumberMessage),
    ControlChange14Bit(ControlChange14BitMessage),
    Tempo(Bpm),
}

impl<M: ShortMessage + ShortMessageFactory + Copy> MidiSourceValue<M> {
    pub fn to_short_messages(&self) -> [Option<M>; 4] {
        match self {
            MidiSourceValue::Plain(msg) => [Some(*msg), None, None, None],
            MidiSourceValue::ParameterNumber(msg) => msg.to_short_messages(),
            MidiSourceValue::ControlChange14Bit(msg) => {
                let inner_shorts = msg.to_short_messages();
                [Some(inner_shorts[0]), Some(inner_shorts[1]), None, None]
            }
            MidiSourceValue::Tempo(_) => [None; 4],
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
        UnitValue::new((self.get() - Bpm::MIN.get()) / Bpm::MAX.get())
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
