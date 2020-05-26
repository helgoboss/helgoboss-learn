use crate::{DiscreteValue, Interval, UnitValue};
use derive_more::Display;
use helgoboss_midi::{ControlChange14BitMessage, ParameterNumberMessage, ShortMessage};

/// Incoming value which might be used to control something
#[derive(Debug, Clone, PartialEq)]
pub enum MidiSourceValue<M: ShortMessage> {
    Plain(M),
    ParameterNumber(ParameterNumberMessage),
    ControlChange14Bit(ControlChange14BitMessage),
    Tempo(Bpm),
}

/// This represents a tempo measured in beats per minute.
#[derive(Copy, Clone, PartialEq, PartialOrd, Debug, Default, Display)]
pub struct Bpm(pub(crate) f64);

impl Bpm {
    /// The minimum possible value (1.0 bpm).
    pub const MIN: Bpm = Bpm(1.0);

    /// The maximum possible value (960.0 bpm).
    pub const MAX: Bpm = Bpm(960.0);

    /// Creates a BPM value.
    ///
    /// # Panics
    ///
    /// This function panics if the given value is not within the BPM range supported by REAPER
    /// `(1.0..=960.0)`.
    pub fn new(value: f64) -> Bpm {
        assert!(Bpm::MIN.get() <= value && value <= Bpm::MAX.get());
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
