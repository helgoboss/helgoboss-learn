use crate::mode::value_sequence::parser::RawEntry;
use crate::UnitValue;
use std::convert::TryInto;
use std::fmt;
use std::fmt::{Display, Formatter, Write};

#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct ValueSequence {
    entries: Vec<ValueSequenceEntry>,
    use_percentages: bool,
}

impl ValueSequence {
    pub fn parse<C: ValueSequenceIo>(context: &C, input: &str) -> Result<Self, &'static str> {
        let (_, raw_sequence) =
            super::parser::parse_value_sequence(input).map_err(|_| "couldn't parse sequence")?;
        let sequence = ValueSequence {
            entries: {
                let result: Result<Vec<_>, &'static str> = raw_sequence
                    .entries
                    .iter()
                    .map(|e| match e {
                        RawEntry::SingleValue(e) => {
                            let val = context.parse(raw_sequence.use_percentages, e)?;
                            Ok(ValueSequenceEntry::SingleValue(val))
                        }
                        RawEntry::Range(e) => {
                            let entry = ValueSequenceRangeEntry {
                                min: UnitValue::MIN,
                                max: UnitValue::MIN,
                                step_size: None,
                            };
                            Ok(ValueSequenceEntry::Range(entry))
                        }
                    })
                    .collect();
                let result = result?;
                result
            },
            use_percentages: raw_sequence.use_percentages,
        };
        Ok(sequence)
    }

    pub fn entries(&self) -> &[ValueSequenceEntry] {
        &self.entries
    }

    pub fn use_percentages(&self) -> bool {
        self.use_percentages
    }

    pub fn in_context<'a, C: ValueSequenceIo>(
        &'a self,
        context: &'a C,
    ) -> WithContext<'a, Self, C> {
        WithContext::new(self, self.use_percentages, context)
    }
}

pub struct WithContext<'a, A, C> {
    actual: &'a A,
    use_percentages: bool,
    context: &'a C,
}

impl<'a, A, C> WithContext<'a, A, C> {
    fn new(actual: &'a A, use_percentages: bool, context: &'a C) -> Self {
        WithContext {
            actual,
            use_percentages,
            context,
        }
    }
}

impl<'a, C: DefaultStepSize> WithContext<'a, ValueSequence, C> {
    pub fn unpack(&self) -> Vec<UnitValue> {
        self.actual
            .entries
            .iter()
            .map(|e| WithContext::new(e, self.use_percentages, self.context))
            .flatten()
            .collect()
    }
}

impl<'a, C: ValueSequenceIo> Display for WithContext<'a, ValueSequence, C> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let snippets: Vec<_> = self
            .actual
            .entries
            .iter()
            .map(|e| WithContext::new(e, self.use_percentages, self.context).to_string())
            .collect();
        let csv = snippets.join(", ");
        write!(f, "{} {}", csv, if self.use_percentages { "%" } else { "" })
    }
}

pub trait ValueSequenceIo {
    fn format(
        &self,
        value: UnitValue,
        use_percentages: bool,
        f: &mut fmt::Formatter,
    ) -> fmt::Result;
    fn parse(&self, use_percentages: bool, text: &str) -> Result<UnitValue, &'static str>;
}

pub trait DefaultStepSize {
    fn default_step_size(&self) -> UnitValue;
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ValueSequenceEntry {
    SingleValue(UnitValue),
    Range(ValueSequenceRangeEntry),
}

impl<'a, C: ValueSequenceIo> Display for WithContext<'a, ValueSequenceEntry, C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ValueSequenceEntry::*;
        match self.actual {
            SingleValue(v) => self.context.format(*v, self.use_percentages, f),
            Range(r) => WithContext::new(r, self.use_percentages, self.context).fmt(f),
        }
    }
}

impl<'a, C: DefaultStepSize> IntoIterator for WithContext<'a, ValueSequenceEntry, C> {
    type Item = UnitValue;
    type IntoIter = ValueSequenceRangeIterator;

    fn into_iter(self) -> ValueSequenceRangeIterator {
        use ValueSequenceEntry::*;
        match self.actual {
            SingleValue(uv) => {
                let simple_range_entry = ValueSequenceRangeEntry {
                    min: *uv,
                    max: *uv,
                    step_size: Some(UnitValue::MAX),
                };
                WithContext::new(&simple_range_entry, self.use_percentages, self.context)
                    .into_iter()
            }
            Range(r) => WithContext::new(r, self.use_percentages, self.context).into_iter(),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct ValueSequenceRangeEntry {
    min: UnitValue,
    max: UnitValue,
    step_size: Option<UnitValue>,
}

impl<'a, C: ValueSequenceIo> Display for WithContext<'a, ValueSequenceRangeEntry, C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.context
            .format(self.actual.min, self.use_percentages, f)?;
        f.write_char('-')?;
        self.context
            .format(self.actual.max, self.use_percentages, f)?;
        if let Some(step_size) = self.actual.step_size {
            f.write_str(" (")?;
            self.context.format(step_size, self.use_percentages, f)?;
            f.write_char(')')?;
        }
        Ok(())
    }
}

impl<'a, C: DefaultStepSize> IntoIterator for WithContext<'a, ValueSequenceRangeEntry, C> {
    type Item = UnitValue;
    type IntoIter = ValueSequenceRangeIterator;

    fn into_iter(self) -> ValueSequenceRangeIterator {
        ValueSequenceRangeIterator {
            val: self.actual.min.get(),
            max: self.actual.max.get(),
            step_size: self
                .actual
                .step_size
                .unwrap_or_else(|| self.context.default_step_size())
                .get(),
        }
    }
}

pub struct ValueSequenceRangeIterator {
    val: f64,
    max: f64,
    step_size: f64,
}

impl Iterator for ValueSequenceRangeIterator {
    type Item = UnitValue;

    fn next(&mut self) -> Option<UnitValue> {
        let val = self.val;
        if val > self.max {
            return None;
        }
        self.val = val + self.step_size;
        Some(val.try_into().ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{format_percentage_without_unit, parse_percentage_without_unit};

    struct TestValueContext;

    impl ValueSequenceIo for TestValueContext {
        fn format(
            &self,
            value: UnitValue,
            use_percentages: bool,
            f: &mut Formatter,
        ) -> fmt::Result {
            f.write_str(&format_percentage_without_unit(value.get()))
        }

        fn parse(&self, use_percentages: bool, text: &str) -> Result<UnitValue, &'static str> {
            parse_percentage_without_unit(text)?.try_into()
        }
    }

    impl DefaultStepSize for TestValueContext {
        fn default_step_size(&self) -> UnitValue {
            UnitValue::new(0.01)
        }
    }

    #[test]
    fn basic() {
        // Given
        let sequence = ValueSequence::parse(&TestValueContext, "25, 50, 75, 50, 100 %").unwrap();
        // When
        // Then
        assert!(sequence.use_percentages());
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
            sequence.in_context(&TestValueContext).unpack(),
            vec![uv(0.25), uv(0.50), uv(0.75), uv(0.50), uv(1.00)]
        );
        assert_eq!(
            &sequence.in_context(&TestValueContext).to_string(),
            "25, 50, 75, 50, 100 %"
        );
    }

    fn uv(value: f64) -> UnitValue {
        UnitValue::new(value)
    }
}
