use crate::UnitValue;
use std::convert::TryInto;
use std::fmt;
use std::fmt::{Display, Formatter, Write};

#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct ValueSequence {
    entries: Vec<ValueSequenceEntry>,
    unit: String,
}

impl ValueSequence {
    pub fn parse<C: ValueContext>(&self, context: &C) -> Result<Self, &'static str> {
        todo!()
    }

    pub fn entries(&self) -> &[ValueSequenceEntry] {
        &self.entries
    }

    pub fn unit(&self) -> &str {
        &self.unit
    }

    pub fn in_context<'a, C: ValueContext>(&'a self, context: &'a C) -> WithContext<'a, Self, C> {
        WithContext::new(self, &self.unit, context)
    }
}

pub struct WithContext<'a, A, C: ValueContext> {
    actual: &'a A,
    unit: &'a str,
    context: &'a C,
}

impl<'a, A, C: ValueContext> WithContext<'a, A, C> {
    fn new(actual: &'a A, unit: &'a str, context: &'a C) -> Self {
        WithContext {
            actual,
            unit,
            context,
        }
    }
}

impl<'a, C: ValueContext> WithContext<'a, ValueSequence, C> {
    pub fn unpack(&self) -> Vec<UnitValue> {
        self.actual
            .entries
            .iter()
            .map(|e| WithContext::new(e, self.unit, self.context))
            .flatten()
            .collect()
    }
}

impl<'a, C: ValueContext> Display for WithContext<'a, ValueSequence, C> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let snippets: Vec<_> = self
            .actual
            .entries
            .iter()
            .map(|e| WithContext::new(e, self.unit, self.context).to_string())
            .collect();
        let csv = snippets.join(", ");
        write!(f, "{}{}", csv, self.unit)
    }
}

pub trait ValueContext {
    fn format(&self, value: UnitValue, unit: &str, f: &mut fmt::Formatter) -> fmt::Result;
    fn parse(&self, unit: &str, text: &str) -> Result<UnitValue, &'static str>;
    fn default_step_size(&self) -> UnitValue;
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ValueSequenceEntry {
    SingleValue(UnitValue),
    Range(ValueSequenceRangeEntry),
}

impl<'a, C: ValueContext> Display for WithContext<'a, ValueSequenceEntry, C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ValueSequenceEntry::*;
        match self.actual {
            SingleValue(v) => self.context.format(*v, self.unit, f),
            Range(r) => WithContext::new(r, self.unit, self.context).fmt(f),
        }
    }
}

impl<'a, C: ValueContext> IntoIterator for WithContext<'a, ValueSequenceEntry, C> {
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
                WithContext::new(&simple_range_entry, self.unit, self.context).into_iter()
            }
            Range(r) => WithContext::new(r, self.unit, self.context).into_iter(),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct ValueSequenceRangeEntry {
    min: UnitValue,
    max: UnitValue,
    step_size: Option<UnitValue>,
}

impl<'a, C: ValueContext> Display for WithContext<'a, ValueSequenceRangeEntry, C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.context.format(self.actual.min, self.unit, f)?;
        f.write_char('-')?;
        self.context.format(self.actual.max, self.unit, f)?;
        if let Some(step_size) = self.actual.step_size {
            f.write_str(" (")?;
            self.context.format(step_size, self.unit, f)?;
            f.write_char(')')?;
        }
        Ok(())
    }
}

impl<'a, C: ValueContext> IntoIterator for WithContext<'a, ValueSequenceRangeEntry, C> {
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

    impl ValueContext for TestValueContext {
        fn format(&self, value: UnitValue, unit: &str, f: &mut Formatter) -> fmt::Result {
            f.write_str(&format_percentage_without_unit(value.get()))
        }

        fn parse(&self, unit: &str, text: &str) -> Result<UnitValue, &'static str> {
            let percentage = parse_percentage_without_unit(text)?;
            (percentage / 100.0).try_into()
        }

        fn default_step_size(&self) -> UnitValue {
            UnitValue::new(0.01)
        }
    }

    #[test]
    fn basic() {
        // Given
        // let sequence: ValueSequence = "25, 50, 75, 50, 100 %".parse().unwrap();
        let sequence = ValueSequence {
            entries: vec![
                ValueSequenceEntry::SingleValue(uv(0.25)),
                ValueSequenceEntry::SingleValue(uv(0.50)),
                ValueSequenceEntry::SingleValue(uv(0.75)),
                ValueSequenceEntry::SingleValue(uv(0.50)),
                ValueSequenceEntry::SingleValue(uv(1.00)),
            ],
            unit: "%".to_string(),
        };

        // When
        // Then
        assert_eq!(sequence.unit(), "%");
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
            "25, 50, 75, 50, 100%"
        );
    }

    fn uv(value: f64) -> UnitValue {
        UnitValue::new(value)
    }
}
