use helgoboss_midi::SevenBitValue;

pub enum ControlValue {
    Absolute(AbsoluteControlValue),
    Relative(RelativeControlValue),
}

pub struct AbsoluteControlValue(pub f64);

pub struct RelativeControlValue(pub i8);

impl RelativeControlValue {
    pub fn from_encoder_1_value(value: SevenBitValue) -> RelativeControlValue {
        debug_assert!(value < 128);
        // 127 = decrement; 0 = none; 1 = increment
        // 127 > value > 63 results in higher decrement step sizes (64 possible decrement step
        // sizes) 1 < value <= 63 results in higher increment step sizes (63
        // possible increment step sizes)
        let increment = if value <= 63 {
            // Zero and increment
            value as i8
        } else {
            // Decrement
            -1 * (128 - value) as i8
        };
        RelativeControlValue(increment)
    }

    pub fn from_encoder_2_value(value: SevenBitValue) -> RelativeControlValue {
        debug_assert!(value < 128);
        // 63 = decrement; 64 = none; 65 = increment
        // 63 > value >= 0 results in higher decrement step sizes (64 possible decrement step
        // sizes) 65 < value <= 127 results in higher increment step sizes (63
        // possible increment step sizes)
        let increment = if value >= 64 {
            // Zero and increment
            (value - 64) as i8
        } else {
            // Decrement
            -1 * (64 - value) as i8
        };
        RelativeControlValue(increment)
    }

    pub fn from_encoder_3_value(value: SevenBitValue) -> RelativeControlValue {
        debug_assert!(value < 128);
        // 65 = decrement; 0 = none; 1 = increment
        // 65 < value <= 127 results in higher decrement step sizes (63 possible decrement step
        // sizes) 1 < value <= 64 results in higher increment step sizes (64 possible
        // increment step sizes)
        let increment = if value <= 64 {
            // Zero and increment
            value as i8
        } else {
            // Decrement
            -1 * (value - 64) as i8
        };
        RelativeControlValue(increment)
    }
}
