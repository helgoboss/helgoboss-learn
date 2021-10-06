use crate::{AbsoluteValue, Fraction, RawMidiEvent, UnitValue};
use logos::{Lexer, Logos};
use std::fmt;
use std::fmt::{Display, Formatter, Write};
use std::str::FromStr;

#[derive(Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct RawMidiPattern {
    entries: Vec<RawMidiPatternEntry>,
    resolution: u8,
}

impl RawMidiPattern {
    pub fn from_entries(entries: Vec<RawMidiPatternEntry>) -> Self {
        let max_variable_bit_index = entries
            .iter()
            .filter_map(|e| e.max_variable_bit_index())
            .max();
        Self {
            entries,
            resolution: if let Some(i) = max_variable_bit_index {
                i + 1
            } else {
                0
            },
        }
    }

    pub fn fixed_from_slice(bytes: &[u8]) -> Self {
        let entries = bytes
            .iter()
            .map(|byte| RawMidiPatternEntry::FixedByte(*byte))
            .collect();
        Self {
            entries,
            resolution: 0,
        }
    }

    pub fn entries(&self) -> &[RawMidiPatternEntry] {
        &self.entries
    }

    /// Resolution in bit (maximum 16 bit).
    ///
    /// If no variable bytes, this returns 0.
    pub fn resolution(&self) -> u8 {
        self.resolution
    }

    /// If no variable bytes, this returns 0.
    pub fn max_discrete_value(&self) -> u16 {
        (2u32.pow(self.resolution as _) - 1) as u16
    }

    pub fn step_size(&self) -> Option<UnitValue> {
        let max = self.max_discrete_value();
        if max == 0 {
            return None;
        }
        Some(UnitValue::new_clamped(1.0 / max as f64))
    }

    /// If it matches and there are no variable bytes in the pattern, this returns
    /// `Some(Fraction(0, 0))`.
    pub fn match_and_capture(&self, bytes: &[u8]) -> Option<Fraction> {
        if bytes.len() != self.entries.len() {
            return None;
        }
        let mut current_value: u16 = 0;
        for (i, b) in bytes.iter().enumerate() {
            let pattern_entry = self.entries[i];
            if let Some(v) = pattern_entry.match_and_capture(*b, current_value) {
                current_value = v;
            } else {
                return None;
            }
        }
        let fraction = Fraction::new(current_value as _, self.max_discrete_value() as _);
        Some(fraction)
    }

    pub fn to_bytes(&self, variable_value: AbsoluteValue) -> Vec<u8> {
        self.byte_iter(variable_value).collect()
    }

    pub fn byte_iter(
        &self,
        variable_value: AbsoluteValue,
    ) -> impl Iterator<Item = u8> + ExactSizeIterator + '_ {
        let discrete_value = match variable_value {
            AbsoluteValue::Continuous(v) => v.to_discrete(self.max_discrete_value()),
            AbsoluteValue::Discrete(f) => {
                std::cmp::min(f.actual(), self.max_discrete_value() as u32) as u16
            }
        };
        self.entries.iter().map(move |e| e.to_byte(discrete_value))
    }

    pub fn to_concrete_midi_event(&self, variable_value: AbsoluteValue) -> RawMidiEvent {
        // TODO-medium Use RawMidiEvent::try_from_iter
        let mut array = [0; RawMidiEvent::MAX_LENGTH];
        let mut i = 0u32;
        for byte in self
            .byte_iter(variable_value)
            .take(RawMidiEvent::MAX_LENGTH)
        {
            array[i as usize] = byte;
            i += 1;
        }
        RawMidiEvent::new(0, i, array)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum RawMidiPatternEntry {
    FixedByte(u8),
    PotentiallyVariableByte(BitPattern),
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct BitPattern {
    /// From most significant to least significant bit.
    entries: [BitPatternEntry; 8],
}

impl BitPattern {
    pub fn contains_variable_portions(&self) -> bool {
        self.entries
            .iter()
            .any(|bpe| matches!(bpe, BitPatternEntry::VariableBit(_)))
    }

    fn max_variable_bit_index(&self) -> Option<u8> {
        self.entries
            .iter()
            .filter_map(|bpe| bpe.variable_bit_index())
            .max()
    }

    pub fn to_byte(self, discrete_value: u16) -> u8 {
        let mut final_byte: u8 = 0;
        for i in 0..8 {
            use BitPatternEntry::*;
            let final_bit = match self.entries[i] {
                FixedBit(bit) => bit,
                VariableBit(bit_index) => (discrete_value & (1 << bit_index) as u16) > 0,
            };
            if final_bit {
                final_byte |= 1 << (7 - i);
            }
        }
        final_byte
    }

    fn match_and_capture(&self, actual_byte: u8, current_value: u16) -> Option<u16> {
        let mut new_value = current_value;
        for i in 0..8 {
            let actual_bit = (actual_byte >> (7 - i)) & 1 == 1;
            use BitPatternEntry::*;
            match self.entries[i] {
                FixedBit(bit) => {
                    if bit != actual_bit {
                        return None;
                    }
                }
                VariableBit(bit_index) => {
                    if actual_bit {
                        new_value |= 1 << bit_index;
                    }
                }
            };
        }
        Some(new_value)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum BitPatternEntry {
    FixedBit(bool),
    /// The number represents the bit index starting from 0 where 0 represents the *least*
    /// significant bit!.
    VariableBit(u8),
}

impl Default for BitPatternEntry {
    fn default() -> Self {
        BitPatternEntry::FixedBit(false)
    }
}

impl BitPatternEntry {
    fn variable_bit_index(&self) -> Option<u8> {
        use BitPatternEntry::*;
        match self {
            FixedBit(_) => None,
            VariableBit(i) => Some(*i),
        }
    }
}

impl RawMidiPatternEntry {
    fn match_and_capture(&self, actual_byte: u8, current_value: u16) -> Option<u16> {
        use RawMidiPatternEntry::*;
        match self {
            FixedByte(b) => {
                if actual_byte == *b {
                    Some(current_value)
                } else {
                    None
                }
            }
            PotentiallyVariableByte(pattern) => {
                pattern.match_and_capture(actual_byte, current_value)
            }
        }
    }

    fn max_variable_bit_index(&self) -> Option<u8> {
        use RawMidiPatternEntry::*;
        match self {
            FixedByte(_) => None,
            PotentiallyVariableByte(bit_pattern) => bit_pattern.max_variable_bit_index(),
        }
    }

    fn to_byte(self, discrete_value: u16) -> u8 {
        use RawMidiPatternEntry::*;
        match self {
            FixedByte(byte) => byte,
            PotentiallyVariableByte(bit_pattern) => bit_pattern.to_byte(discrete_value),
        }
    }
}

impl Display for RawMidiPattern {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let string_vec: Vec<_> = self.entries.iter().map(|e| e.to_string()).collect();
        f.write_str(&string_vec.join(" "))
    }
}

impl Display for RawMidiPatternEntry {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        use RawMidiPatternEntry::*;
        match self {
            FixedByte(byte) => write!(f, "{:02X}", *byte),
            PotentiallyVariableByte(pattern) => write!(f, "[{}]", pattern),
        }
    }
}

impl Display for BitPattern {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        for entry in &self.entries[..4] {
            let _ = entry.fmt(f);
        }
        let _ = f.write_char(' ');
        for entry in &self.entries[4..] {
            let _ = entry.fmt(f);
        }
        Ok(())
    }
}

impl Display for BitPatternEntry {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        use BitPatternEntry::*;
        match self {
            FixedBit(bit) => write!(f, "{}", if *bit { '1' } else { '0' }),
            VariableBit(bit_index) => write!(f, "{}", (97 + bit_index) as char),
        }
    }
}

impl FromStr for RawMidiPattern {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lex: Lexer<RawMidiPatternToken> = RawMidiPatternToken::lexer(s);
        use RawMidiPatternToken::*;
        let entries: Result<Vec<_>, _> = lex
            .map(|token| match token {
                FixedByte(byte) => Ok(RawMidiPatternEntry::FixedByte(byte)),
                Error => Err("invalid pattern expression"),
                PotentiallyVariableByte(pattern) => {
                    Ok(RawMidiPatternEntry::PotentiallyVariableByte(pattern))
                }
            })
            .collect();
        let p = RawMidiPattern::from_entries(entries?);
        Ok(p)
    }
}

#[derive(Logos, Debug, PartialEq)]
enum RawMidiPatternToken {
    #[regex(r"\[[01abcdefghijklmnop ]*\]", parse_as_bit_pattern)]
    PotentiallyVariableByte(BitPattern),
    #[regex(r"[0-9a-fA-F][0-9a-fA-F]?", parse_as_byte)]
    FixedByte(u8),
    #[error]
    #[regex(r"[ \t\n\f]+", logos::skip)]
    Error,
}

fn parse_as_byte(lex: &mut Lexer<RawMidiPatternToken>) -> Result<u8, core::num::ParseIntError> {
    u8::from_str_radix(lex.slice(), 16)
}

fn parse_as_bit_pattern(lex: &mut Lexer<RawMidiPatternToken>) -> Result<BitPattern, &'static str> {
    let mut entries: [BitPatternEntry; 8] = Default::default();
    let slice: &str = lex.slice();
    let mut i = 0;
    for c in slice.chars() {
        use BitPatternEntry::*;
        let entry = match c {
            '0' => FixedBit(false),
            '1' => FixedBit(true),
            'a'..='p' => VariableBit(c as u8 - 97),
            _ => continue,
        };
        if i > 7 {
            return Err("too many bits in bit pattern");
        }
        entries[i] = entry;
        i += 1;
    }
    let pattern = BitPattern { entries };
    Ok(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_variable_nibble() {
        // Given
        let pattern: RawMidiPattern = "F0 [0000 dcba] F7".parse().unwrap();
        // When
        // Then
        assert_eq!(
            pattern.to_bytes(AbsoluteValue::Continuous(UnitValue::MAX)),
            vec![0xf0, 0x0f, 0xf7]
        );
        assert_eq!(
            pattern.match_and_capture(&[0xf0, 0x0f, 0xf7]),
            Some(Fraction::new(15, 15))
        );
        assert_eq!(
            pattern.to_bytes(AbsoluteValue::Continuous(UnitValue::MIN)),
            vec![0xf0, 0x00, 0xf7]
        );
        assert_eq!(
            pattern.match_and_capture(&[0xf0, 0x00, 0xf7]),
            Some(Fraction::new(0, 15))
        );
        assert_eq!(
            pattern.to_bytes(AbsoluteValue::Continuous(UnitValue::new(0.5))),
            vec![0xf0, 0x08, 0xf7]
        );
        assert_eq!(
            pattern.match_and_capture(&[0xf0, 0x08, 0xf7]),
            Some(Fraction::new(8, 15))
        );
        assert_eq!(&pattern.to_string(), "F0 [0000 dcba] F7");
        assert_eq!(pattern.match_and_capture(&[0xf1, 0x0f, 0xf7]), None);
    }

    #[test]
    fn one_variable_nibble_no_spaces() {
        // Given
        let pattern: RawMidiPattern = "F0[0000dcba]F7".parse().unwrap();
        // When
        // Then
        assert_eq!(
            pattern.to_bytes(AbsoluteValue::Continuous(UnitValue::MAX)),
            vec![0xf0, 0x0f, 0xf7]
        );
        assert_eq!(
            pattern.to_bytes(AbsoluteValue::Continuous(UnitValue::MIN)),
            vec![0xf0, 0x00, 0xf7]
        );
        assert_eq!(
            pattern.to_bytes(AbsoluteValue::Continuous(UnitValue::new(0.5))),
            vec![0xf0, 0x08, 0xf7]
        );
        assert_eq!(&pattern.to_string(), "F0 [0000 dcba] F7");
    }

    #[test]
    fn one_variable_nibble_variation() {
        // Given
        let pattern: RawMidiPattern = "F0[1111dcba]F7".parse().unwrap();
        // When
        // Then
        assert_eq!(
            pattern.to_bytes(AbsoluteValue::Continuous(UnitValue::MAX)),
            vec![0xf0, 0xff, 0xf7]
        );
        assert_eq!(
            pattern.match_and_capture(&[0xf0, 0x0ff, 0xf7]),
            Some(Fraction::new(15, 15))
        );
        assert_eq!(
            pattern.to_bytes(AbsoluteValue::Continuous(UnitValue::MIN)),
            vec![0xf0, 0xf0, 0xf7]
        );
        assert_eq!(
            pattern.match_and_capture(&[0xf0, 0x0f0, 0xf7]),
            Some(Fraction::new(0, 15))
        );
        assert_eq!(
            pattern.to_bytes(AbsoluteValue::Continuous(UnitValue::new(0.5))),
            vec![0xf0, 0xf8, 0xf7]
        );
        assert_eq!(
            pattern.match_and_capture(&[0xf0, 0x0f8, 0xf7]),
            Some(Fraction::new(8, 15))
        );
        assert_eq!(&pattern.to_string(), "F0 [1111 dcba] F7");
    }

    #[test]
    fn wrong_variable_pattern() {
        let result = "F0[0000dcbaa]F7".parse::<RawMidiPattern>();
        assert!(result.is_err());
    }

    #[test]
    fn correct_resolution_1() {
        // Given
        let pattern: RawMidiPattern = "B0 00 [0nml kjih]".parse().unwrap();
        // When
        // Then
        assert_eq!(pattern.resolution(), 14);
    }

    #[test]
    fn correct_resolution_2() {
        // Given
        let pattern: RawMidiPattern = "B0 00 [0gfe dcba]".parse().unwrap();
        // When
        // Then
        assert_eq!(pattern.resolution(), 7);
    }

    #[test]
    fn fixed_pattern() {
        // Given
        let pattern: RawMidiPattern = "B0 00 F7".parse().unwrap();
        // When
        // Then
        assert_eq!(pattern.resolution(), 0);
        assert_eq!(pattern.max_discrete_value(), 0);
        assert_eq!(pattern.match_and_capture(&[0xf0, 0x0f8, 0xf7]), None);
        assert_eq!(
            pattern.match_and_capture(&[0xb0, 0x00, 0xf7]),
            Some(Fraction::new(0, 0))
        );
    }

    #[test]
    fn real_world_fixed_pattern() {
        // Given
        let pattern: RawMidiPattern = "F0 0 20 6B 7F 42 02 00 0 2F 7F F7".parse().unwrap();
        // When
        // Then
        assert_eq!(pattern.resolution(), 0);
        assert_eq!(pattern.max_discrete_value(), 0);
        assert_eq!(pattern.match_and_capture(&[0xf0, 0x0f8, 0xf7]), None);
        assert_eq!(
            pattern.match_and_capture(&[
                0xF0, 0x0, 0x20, 0x6B, 0x7F, 0x42, 0x2, 0x0, 0x0, 0x2F, 0x7F, 0xF6
            ]),
            None
        );
        assert_eq!(
            pattern.match_and_capture(&[
                0xF0, 0x0, 0x20, 0x6B, 0x7F, 0x42, 0x2, 0x0, 0x0, 0x2F, 0x7F, 0xF7
            ]),
            Some(Fraction::new(0, 0))
        );
    }
}
