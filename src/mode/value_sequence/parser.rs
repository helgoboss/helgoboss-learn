use nom::branch::alt;
use nom::character::complete::{space0, space1};
use nom::combinator::opt;
use nom::multi::separated_list0;
use nom::sequence::separated_pair;
use nom::{
    bytes::complete::is_not, character::complete::char, sequence::delimited, sequence::tuple,
    IResult,
};

fn parse_value(input: &str) -> IResult<&str, &str> {
    let parser = is_not("(), ");
    parser(input)
}

fn parse_step_size(input: &str) -> IResult<&str, &str> {
    delimited(
        tuple((char('('), space0)),
        parse_value,
        tuple((space0, char(')'))),
    )(input)
}

fn parse_simple_range(input: &str) -> IResult<&str, RawSimpleRange> {
    let mut parser = separated_pair(parse_value, tuple((space1, char('-'), space1)), parse_value);
    let (remainder, (from, to)) = parser(input)?;
    Ok((remainder, RawSimpleRange { from, to }))
}

fn parse_full_range(input: &str) -> IResult<&str, RawFullRange> {
    let mut parser = tuple((parse_simple_range, space0, opt(parse_step_size)));
    let (remainder, (simple_range, _, step_size)) = parser(input)?;
    Ok((remainder, RawFullRange::new(simple_range, step_size)))
}

fn parse_range_entry(input: &str) -> IResult<&str, RawEntry> {
    let (remainder, range) = parse_full_range(input)?;
    Ok((remainder, RawEntry::Range(range)))
}

fn parse_single_value_entry(input: &str) -> IResult<&str, RawEntry> {
    let (remainder, single_value) = parse_value(input)?;
    Ok((remainder, RawEntry::SingleValue(single_value)))
}

fn parse_entry(input: &str) -> IResult<&str, RawEntry> {
    let mut parser = alt((parse_range_entry, parse_single_value_entry));
    parser(input)
}

pub fn parse_entries(input: &str) -> IResult<&str, Vec<RawEntry>> {
    let mut parser = separated_list0(tuple((space0, char(','), space0)), parse_entry);
    parser(input)
}

#[derive(Eq, PartialEq, Debug)]
pub enum RawEntry<'a> {
    SingleValue(&'a str),
    Range(RawFullRange<'a>),
}

#[derive(Eq, PartialEq, Debug)]
pub struct RawFullRange<'a> {
    pub simple_range: RawSimpleRange<'a>,
    pub step_size: Option<&'a str>,
}

impl<'a> RawFullRange<'a> {
    fn new(simple_range: RawSimpleRange<'a>, step_size: Option<&'a str>) -> Self {
        Self {
            simple_range,
            step_size,
        }
    }
}

#[derive(Eq, PartialEq, Debug)]
pub struct RawSimpleRange<'a> {
    pub from: &'a str,
    pub to: &'a str,
}

#[allow(clippy::needless_lifetimes)]
impl<'a> RawSimpleRange<'a> {
    #[cfg(test)]
    fn new(from: &'a str, to: &'a str) -> Self {
        Self { from, to }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_size() {
        assert_eq!(parse_step_size("(0.5)"), Ok(("", "0.5")));
        assert_eq!(parse_step_size("(8)"), Ok(("", "8")));
        assert_eq!(parse_step_size("( abc )"), Ok(("", "abc")));
    }

    #[test]
    fn simple_range() {
        assert_eq!(
            parse_simple_range("5 - 10"),
            Ok(("", RawSimpleRange::new("5", "10")))
        );
        assert_eq!(
            parse_simple_range("5.0 - 10.0"),
            Ok(("", RawSimpleRange::new("5.0", "10.0")))
        );
        assert_eq!(
            parse_simple_range("a - f"),
            Ok(("", RawSimpleRange::new("a", "f")))
        );
    }

    #[test]
    fn full_range() {
        assert_eq!(
            parse_full_range("5 - 10"),
            Ok(("", RawFullRange::new(RawSimpleRange::new("5", "10"), None)))
        );
        assert_eq!(
            parse_full_range("5 - 10 (0.1)"),
            Ok((
                "",
                RawFullRange::new(RawSimpleRange::new("5", "10"), Some("0.1"))
            ))
        );
    }

    #[test]
    fn entry() {
        assert_eq!(
            parse_entry("5 - 10 (0.1)"),
            Ok((
                "",
                RawEntry::Range(RawFullRange::new(
                    RawSimpleRange::new("5", "10"),
                    Some("0.1")
                ))
            ))
        );
        assert_eq!(parse_entry("75.5"), Ok(("", RawEntry::SingleValue("75.5"))));
    }

    #[test]
    fn entries() {
        assert_eq!(
            parse_entries("5 - 10 (0.1), 12.5, 15 - 20"),
            Ok((
                "",
                vec![
                    RawEntry::Range(RawFullRange::new(
                        RawSimpleRange::new("5", "10"),
                        Some("0.1")
                    )),
                    RawEntry::SingleValue("12.5"),
                    RawEntry::Range(RawFullRange::new(RawSimpleRange::new("15", "20"), None))
                ]
            ))
        );
    }
}
