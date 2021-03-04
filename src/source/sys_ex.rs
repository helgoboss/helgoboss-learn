use crate::UnitValue;
use logos::{Lexer, Logos};
use std::fmt;
use std::fmt::{Display, Formatter, Write};
use std::str::FromStr;

#[derive(Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct SysExPattern {
    entries: Vec<SysExPatternEntry>,
    resolution: u8,
}

impl SysExPattern {
    pub fn resolution(&self) -> u8 {
        self.resolution
    }

    pub fn max_discrete_value(&self) -> u16 {
        2_u16.pow(self.resolution as _) - 1
    }

    pub fn from_entries(entries: Vec<SysExPatternEntry>) -> Self {
        let max_variable_bit_index = entries
            .iter()
            .map(|e| e.max_variable_bit_index())
            .max()
            .unwrap_or(0);
        Self {
            entries,
            resolution: max_variable_bit_index + 1,
        }
    }

    pub fn fixed_from_slice(bytes: &[u8]) -> Self {
        let entries = bytes
            .iter()
            .map(|byte| SysExPatternEntry::FixedByte(*byte))
            .collect();
        Self {
            entries,
            resolution: 0,
        }
    }

    pub fn to_bytes(&self, variable_value: UnitValue) -> Vec<u8> {
        self.byte_iter(variable_value).collect()
    }

    pub fn byte_iter(
        &self,
        variable_value: UnitValue,
    ) -> impl Iterator<Item = u8> + ExactSizeIterator + '_ {
        let discrete_value = variable_value.to_discrete(self.max_discrete_value());
        self.entries.iter().map(move |e| e.to_byte(discrete_value))
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum SysExPatternEntry {
    FixedByte(u8),
    VariableByte(BitPattern),
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct BitPattern {
    /// From most significant to least significant bit.
    entries: [BitPatternEntry; 8],
}

impl BitPattern {
    fn max_variable_bit_index(&self) -> u8 {
        self.entries
            .iter()
            .map(|bpe| bpe.variable_bit_index())
            .max()
            .unwrap()
    }

    fn to_byte(&self, discrete_value: u16) -> u8 {
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
    fn variable_bit_index(&self) -> u8 {
        use BitPatternEntry::*;
        match self {
            FixedBit(_) => 0,
            VariableBit(i) => *i,
        }
    }
}

impl SysExPatternEntry {
    fn max_variable_bit_index(&self) -> u8 {
        use SysExPatternEntry::*;
        match self {
            FixedByte(_) => 0u8,
            VariableByte(bit_pattern) => bit_pattern.max_variable_bit_index(),
        }
    }

    fn to_byte(&self, discrete_value: u16) -> u8 {
        use SysExPatternEntry::*;
        match self {
            FixedByte(byte) => *byte,
            VariableByte(bit_pattern) => bit_pattern.to_byte(discrete_value),
        }
    }
}

impl Display for SysExPattern {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let string_vec: Vec<_> = self.entries.iter().map(|e| e.to_string()).collect();
        f.write_str(&string_vec.join(" "))
    }
}

impl Display for SysExPatternEntry {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        use SysExPatternEntry::*;
        match self {
            FixedByte(byte) => write!(f, "{:X}", *byte),
            VariableByte(pattern) => write!(f, "[{}]", pattern),
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

impl FromStr for SysExPattern {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lex: Lexer<SysExPatternToken> = SysExPatternToken::lexer(s);
        use SysExPatternToken::*;
        let entries: Result<Vec<_>, _> = lex
            .map(|token| match token {
                FixedByte(byte) => Ok(SysExPatternEntry::FixedByte(byte)),
                Error => return Err("invalid pattern expression"),
                VariableByte(pattern) => Ok(SysExPatternEntry::VariableByte(pattern)),
            })
            .collect();
        let p = SysExPattern::from_entries(entries?);
        Ok(p)
    }
}
#[derive(Logos, Debug, PartialEq)]
enum SysExPatternToken {
    #[regex(r"\[[01abcdefghijklmnop ]*\]", parse_as_bit_pattern)]
    VariableByte(BitPattern),
    #[regex(r"[0-9a-fA-F][0-9a-fA-F]", parse_as_byte)]
    FixedByte(u8),
    #[error]
    #[regex(r"[ \t\n\f]+", logos::skip)]
    Error,
}

fn parse_as_byte(lex: &mut Lexer<SysExPatternToken>) -> Result<u8, core::num::ParseIntError> {
    u8::from_str_radix(lex.slice(), 16)
}

fn parse_as_bit_pattern(lex: &mut Lexer<SysExPatternToken>) -> Result<BitPattern, &'static str> {
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
        let pattern: SysExPattern = "F0 [0000 dcba] F7".parse().unwrap();
        // When
        // Then
        assert_eq!(pattern.to_bytes(UnitValue::MAX), vec![0xf0, 0x0f, 0xf7]);
        assert_eq!(pattern.to_bytes(UnitValue::MIN), vec![0xf0, 0x00, 0xf7]);
        assert_eq!(
            pattern.to_bytes(UnitValue::new(0.5)),
            vec![0xf0, 0x08, 0xf7]
        );
        assert_eq!(&pattern.to_string(), "F0 [0000 dcba] F7");
    }

    #[test]
    fn one_variable_nibble_no_spaces() {
        // Given
        let pattern: SysExPattern = "F0[0000dcba]F7".parse().unwrap();
        // When
        // Then
        assert_eq!(pattern.to_bytes(UnitValue::MAX), vec![0xf0, 0x0f, 0xf7]);
        assert_eq!(pattern.to_bytes(UnitValue::MIN), vec![0xf0, 0x00, 0xf7]);
        assert_eq!(
            pattern.to_bytes(UnitValue::new(0.5)),
            vec![0xf0, 0x08, 0xf7]
        );
        assert_eq!(&pattern.to_string(), "F0 [0000 dcba] F7");
    }

    #[test]
    fn one_variable_nibble_variation() {
        // Given
        let pattern: SysExPattern = "F0[1111dcba]F7".parse().unwrap();
        // When
        // Then
        assert_eq!(pattern.to_bytes(UnitValue::MAX), vec![0xf0, 0xff, 0xf7]);
        assert_eq!(pattern.to_bytes(UnitValue::MIN), vec![0xf0, 0xf0, 0xf7]);
        assert_eq!(
            pattern.to_bytes(UnitValue::new(0.5)),
            vec![0xf0, 0xf8, 0xf7]
        );
        assert_eq!(&pattern.to_string(), "F0 [1111 dcba] F7");
    }

    #[test]
    fn wrong_variable_pattern() {
        let result = "F0[0000dcbaa]F7".parse::<SysExPattern>();
        assert!(result.is_err());
    }
}
