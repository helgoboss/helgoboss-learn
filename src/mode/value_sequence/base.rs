use crate::mode::value_sequence::parser::RawEntry;
use crate::{
    format_percentage_without_unit, parse_percentage_without_unit, UnitValue, BASE_EPSILON,
};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use std::convert::TryInto;
use std::fmt;
use std::fmt::{Debug, Display, Formatter, Write};

#[derive(Clone, Eq, PartialEq, Debug, Default, SerializeDisplay, DeserializeFromStr)]
pub struct ValueSequence {
    entries: Vec<ValueSequenceEntry>,
}

impl ValueSequence {
    pub fn parse<P: ValueParser>(
        single_value_parser: &P,
        input: &str,
    ) -> Result<Self, &'static str> {
        let (_, raw_entries) =
            super::parser::parse_entries(input).map_err(|_| "couldn't parse sequence")?;
        let sequence = ValueSequence {
            entries: {
                raw_entries
                    .iter()
                    .map(|e| match e {
                        RawEntry::SingleValue(e) => ValueSequenceEntry::SingleValue(
                            single_value_parser.parse_value(e).unwrap_or_default(),
                        ),
                        RawEntry::Range(e) => {
                            let entry = ValueSequenceRangeEntry {
                                from: single_value_parser
                                    .parse_value(e.simple_range.from)
                                    .unwrap_or_default(),
                                to: single_value_parser
                                    .parse_value(e.simple_range.to)
                                    .unwrap_or_default(),
                                step_size: e
                                    .step_size
                                    .map(|s| single_value_parser.parse_step(s).unwrap_or_default()),
                            };
                            ValueSequenceEntry::Range(entry)
                        }
                    })
                    .collect()
            },
        };
        Ok(sequence)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[ValueSequenceEntry] {
        &self.entries
    }

    pub fn displayable<'a>(&'a self, f: &'a impl ValueFormatter) -> impl Display + 'a {
        DisplayableValueSequence {
            value_sequence: self,
            value_formatter: f,
        }
    }

    pub fn unpack(&self, default_step_size: UnitValue) -> Vec<UnitValue> {
        self.entries
            .iter()
            .flat_map(|e| WithDefaultStepSize::new(e, default_step_size))
            .collect()
    }
}

struct WithDefaultStepSize<'a, A> {
    actual: &'a A,
    default_step_size: UnitValue,
}

impl<'a, A> WithDefaultStepSize<'a, A> {
    fn new(actual: &'a A, default_step_size: UnitValue) -> Self {
        Self {
            actual,
            default_step_size,
        }
    }
}

struct WithFormatter<'a, A, F: ValueFormatter> {
    actual: &'a A,
    value_formatter: &'a F,
}

impl<'a, A, F: ValueFormatter> WithFormatter<'a, A, F> {
    fn new(actual: &'a A, value_formatter: &'a F) -> Self {
        WithFormatter {
            actual,
            value_formatter,
        }
    }
}

struct DisplayableValueSequence<'a, F> {
    value_sequence: &'a ValueSequence,
    value_formatter: &'a F,
}

impl<F: ValueFormatter> Display for DisplayableValueSequence<'_, F> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let snippets: Vec<_> = self
            .value_sequence
            .entries
            .iter()
            .map(|e| WithFormatter::new(e, self.value_formatter).to_string())
            .collect();
        let csv = snippets.join(", ");
        f.write_str(&csv)
    }
}

pub trait ValueFormatter {
    fn format_value(&self, value: UnitValue, f: &mut fmt::Formatter) -> fmt::Result;
    fn format_step(&self, value: UnitValue, f: &mut fmt::Formatter) -> fmt::Result;
}

pub trait ValueParser {
    fn parse_value(&self, text: &str) -> Result<UnitValue, &'static str>;
    fn parse_step(&self, text: &str) -> Result<UnitValue, &'static str>;
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ValueSequenceEntry {
    SingleValue(UnitValue),
    Range(ValueSequenceRangeEntry),
}

impl<F: ValueFormatter> Display for WithFormatter<'_, ValueSequenceEntry, F> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ValueSequenceEntry as E;
        match self.actual {
            E::SingleValue(v) => self.value_formatter.format_value(*v, f),
            E::Range(r) => WithFormatter::new(r, self.value_formatter).fmt(f),
        }
    }
}

impl IntoIterator for WithDefaultStepSize<'_, ValueSequenceEntry> {
    type Item = UnitValue;
    type IntoIter = ValueSequenceRangeIterator;

    fn into_iter(self) -> ValueSequenceRangeIterator {
        use ValueSequenceEntry as E;
        match self.actual {
            E::SingleValue(uv) => {
                let simple_range_entry = ValueSequenceRangeEntry {
                    from: *uv,
                    to: *uv,
                    step_size: Some(UnitValue::MAX),
                };
                WithDefaultStepSize::new(&simple_range_entry, self.default_step_size).into_iter()
            }
            E::Range(r) => WithDefaultStepSize::new(r, self.default_step_size).into_iter(),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct ValueSequenceRangeEntry {
    from: UnitValue,
    to: UnitValue,
    step_size: Option<UnitValue>,
}

impl<F: ValueFormatter> Display for WithFormatter<'_, ValueSequenceRangeEntry, F> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.value_formatter.format_value(self.actual.from, f)?;
        f.write_str(" - ")?;
        self.value_formatter.format_value(self.actual.to, f)?;
        if let Some(step_size) = self.actual.step_size {
            f.write_str(" (")?;
            self.value_formatter.format_step(step_size, f)?;
            f.write_char(')')?;
        }
        Ok(())
    }
}

impl IntoIterator for WithDefaultStepSize<'_, ValueSequenceRangeEntry> {
    type Item = UnitValue;
    type IntoIter = ValueSequenceRangeIterator;

    fn into_iter(self) -> ValueSequenceRangeIterator {
        ValueSequenceRangeIterator {
            i: self.actual.from.get(),
            from: self.actual.from.get(),
            to: self.actual.to.get(),
            step_size: self
                .actual
                .step_size
                .unwrap_or(self.default_step_size)
                .get(),
        }
    }
}

pub struct ValueSequenceRangeIterator {
    i: f64,
    from: f64,
    to: f64,
    step_size: f64,
}

impl Iterator for ValueSequenceRangeIterator {
    type Item = UnitValue;

    fn next(&mut self) -> Option<UnitValue> {
        if self.step_size == 0.0 {
            return None;
        }
        let i = self.i;
        self.i = if self.from <= self.to {
            // Forward
            if i > self.to + BASE_EPSILON {
                return None;
            }
            i + self.step_size
        } else {
            // Backward
            if i + BASE_EPSILON < self.to {
                return None;
            }
            i - self.step_size
        };
        i.try_into().ok()
    }
}

impl Display for ValueSequence {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.displayable(&UnitValueIo).fmt(f)
    }
}

impl std::str::FromStr for ValueSequence {
    type Err = &'static str;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        ValueSequence::parse(&UnitValueIo, input)
    }
}

pub struct UnitValueIo;

impl ValueFormatter for UnitValueIo {
    fn format_value(&self, value: UnitValue, f: &mut Formatter) -> fmt::Result {
        write!(f, "{value}")
    }

    fn format_step(&self, value: UnitValue, f: &mut Formatter) -> fmt::Result {
        self.format_value(value, f)
    }
}

impl ValueParser for UnitValueIo {
    fn parse_value(&self, text: &str) -> Result<UnitValue, &'static str> {
        text.parse()
    }

    fn parse_step(&self, text: &str) -> Result<UnitValue, &'static str> {
        self.parse_value(text)
    }
}

pub struct PercentIo;

impl ValueFormatter for PercentIo {
    fn format_value(&self, value: UnitValue, f: &mut Formatter) -> fmt::Result {
        f.write_str(&format_percentage_without_unit(value.get()))
    }

    fn format_step(&self, value: UnitValue, f: &mut Formatter) -> fmt::Result {
        self.format_value(value, f)
    }
}

impl ValueParser for PercentIo {
    fn parse_value(&self, text: &str) -> Result<UnitValue, &'static str> {
        parse_percentage_without_unit(text)?.try_into()
    }

    fn parse_step(&self, text: &str) -> Result<UnitValue, &'static str> {
        self.parse_value(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    struct TestValueContext;

    impl ValueFormatter for TestValueContext {
        fn format_value(&self, value: UnitValue, f: &mut Formatter) -> fmt::Result {
            write!(f, "{}", (value.get() * 1000.0) as u32)
        }
        fn format_step(&self, value: UnitValue, f: &mut Formatter) -> fmt::Result {
            write!(f, "{}", value)
        }
    }

    impl ValueParser for TestValueContext {
        fn parse_value(&self, text: &str) -> Result<UnitValue, &'static str> {
            let number: u32 = text.parse().map_err(|_| "")?;
            (number as f64 / 1000.0).try_into()
        }

        fn parse_step(&self, text: &str) -> Result<UnitValue, &'static str> {
            text.parse()
        }
    }

    fn default_test_step_size() -> UnitValue {
        UnitValue::new(0.001)
    }

    #[test]
    fn simple_values() {
        // Given
        let sequence = ValueSequence::parse(&TestValueContext, "250, 500, 750, 500, 1000").unwrap();
        // When
        // Then
        assert_eq!(
            sequence.entries(),
            &[
                ValueSequenceEntry::SingleValue(uv(0.25)),
                ValueSequenceEntry::SingleValue(uv(0.50)),
                ValueSequenceEntry::SingleValue(uv(0.75)),
                ValueSequenceEntry::SingleValue(uv(0.50)),
                ValueSequenceEntry::SingleValue(uv(1.00)),
            ]
        );
        assert_eq!(
            sequence.unpack(default_test_step_size()),
            vec![uv(0.25), uv(0.50), uv(0.75), uv(0.50), uv(1.00)]
        );
        assert_eq!(
            &sequence.displayable(&TestValueContext).to_string(),
            "250, 500, 750, 500, 1000"
        );
        assert_eq!(&sequence.to_string(), "0.25, 0.5, 0.75, 0.5, 1")
    }

    #[test]
    fn ranges_native() {
        // Given
        let sequence = ValueSequence::parse(
            &TestValueContext,
            "250 - 255, 500 - 501, 750 - 755 (0.002), 520 - 500(0.01), 999",
        )
        .unwrap();
        // When
        // Then
        assert_eq!(
            sequence.entries(),
            &[
                ValueSequenceEntry::Range(ValueSequenceRangeEntry {
                    from: uv(0.250),
                    to: uv(0.255),
                    step_size: None
                }),
                ValueSequenceEntry::Range(ValueSequenceRangeEntry {
                    from: uv(0.500),
                    to: uv(0.501),
                    step_size: None
                }),
                ValueSequenceEntry::Range(ValueSequenceRangeEntry {
                    from: uv(0.750),
                    to: uv(0.755),
                    step_size: Some(uv(0.002))
                }),
                ValueSequenceEntry::Range(ValueSequenceRangeEntry {
                    from: uv(0.520),
                    to: uv(0.500),
                    step_size: Some(uv(0.010))
                }),
                ValueSequenceEntry::SingleValue(uv(0.999))
            ]
        );
        assert_eq!(
            sequence.unpack(default_test_step_size()),
            vec![
                uv(0.250),
                uv(0.251),
                uv(0.252),
                uv(0.253),
                uv(0.254),
                uv(0.255),
                uv(0.500),
                uv(0.501),
                uv(0.750),
                uv(0.752),
                uv(0.754),
                uv(0.520),
                uv(0.510),
                uv(0.500),
                uv(0.999)
            ]
        );
        assert_eq!(
            &sequence.displayable(&TestValueContext).to_string(),
            "250 - 255, 500 - 501, 750 - 755 (0.002), 520 - 500 (0.01), 999"
        );
        assert_eq!(
            &sequence.to_string(),
            "0.25 - 0.255, 0.5 - 0.501, 0.75 - 0.755 (0.002), 0.52 - 0.5 (0.01), 0.999"
        )
    }

    #[test]
    fn ranges_rounding() {
        // Given
        let sequence = ValueSequence::parse(&PercentIo, "25 - 50, 75, 50, 10").unwrap();
        // When
        let unpacked = sequence.unpack(UnitValue::new(0.01));
        // Then
        assert_eq!(unpacked.len(), 29);
        let at = |i| *unpacked.get(i).unwrap();
        assert_abs_diff_eq!(at(0), uv(0.25));
        assert_abs_diff_eq!(at(1), uv(0.26));
        assert_abs_diff_eq!(at(2), uv(0.27));
        assert_abs_diff_eq!(at(3), uv(0.28));
        assert_abs_diff_eq!(at(4), uv(0.29));
        assert_abs_diff_eq!(at(5), uv(0.30));
        assert_abs_diff_eq!(at(6), uv(0.31));
        assert_abs_diff_eq!(at(7), uv(0.32));
        assert_abs_diff_eq!(at(8), uv(0.33));
        assert_abs_diff_eq!(at(9), uv(0.34));
        assert_abs_diff_eq!(at(10), uv(0.35));
        assert_abs_diff_eq!(at(11), uv(0.36));
        assert_abs_diff_eq!(at(12), uv(0.37));
        assert_abs_diff_eq!(at(13), uv(0.38));
        assert_abs_diff_eq!(at(14), uv(0.39));
        assert_abs_diff_eq!(at(15), uv(0.40));
        assert_abs_diff_eq!(at(16), uv(0.41));
        assert_abs_diff_eq!(at(17), uv(0.42));
        assert_abs_diff_eq!(at(18), uv(0.43));
        assert_abs_diff_eq!(at(19), uv(0.44));
        assert_abs_diff_eq!(at(20), uv(0.45));
        assert_abs_diff_eq!(at(21), uv(0.46));
        assert_abs_diff_eq!(at(22), uv(0.47));
        assert_abs_diff_eq!(at(23), uv(0.48));
        assert_abs_diff_eq!(at(24), uv(0.49));
        assert_abs_diff_eq!(at(25), uv(0.50));
        assert_abs_diff_eq!(at(26), uv(0.75));
        assert_abs_diff_eq!(at(27), uv(0.50));
        assert_abs_diff_eq!(at(28), uv(0.10));
    }

    #[test]
    fn range_corner_cases() {
        // Given
        let sequence =
            ValueSequence::parse(&TestValueContext, "250 - 250, 500 - 501 (0), 601 - 600 (0)")
                .unwrap();
        // When
        // Then
        assert_eq!(sequence.unpack(default_test_step_size()), vec![uv(0.250)]);
    }

    fn uv(value: f64) -> UnitValue {
        UnitValue::new(value)
    }
}
