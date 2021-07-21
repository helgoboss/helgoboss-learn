use nom::branch::alt;
use nom::combinator::opt;
use nom::multi::separated_list0;
use nom::sequence::{pair, separated_pair};
use nom::{
    bytes::complete::is_not, character::complete::char, sequence::delimited, sequence::tuple,
    IResult,
};

fn parse_step_size(input: &str) -> IResult<&str, &str> {
    delimited(char('('), is_not(")"), char(')'))(input)
}

fn parse_single_value(input: &str) -> IResult<&str, &str> {
    let mut parser = is_not("-(),");
    parser(input)
}

fn parse_simple_range(input: &str) -> IResult<&str, RawSimpleRange> {
    let mut parser = separated_pair(parse_single_value, char('-'), parse_single_value);
    let (remainder, (min, max)) = parser(input)?;
    Ok((remainder, RawSimpleRange { min, max }))
}

fn parse_range_entry(input: &str) -> IResult<&str, RawEntry> {
    let (remainder, range) = parse_full_range(input)?;
    Ok((remainder, RawEntry::Range(range)))
}

fn parse_single_value_entry(input: &str) -> IResult<&str, RawEntry> {
    let (remainder, single_value) = parse_single_value(input)?;
    Ok((remainder, RawEntry::SingleValue(single_value)))
}

fn parse_full_range(input: &str) -> IResult<&str, RawFullRange> {
    let mut parser = pair(parse_simple_range, opt(parse_step_size));
    let (remainder, (simple_range, step_size)) = parser(input)?;
    Ok((remainder, RawFullRange::new(simple_range, step_size)))
}

fn parse_entry(input: &str) -> IResult<&str, RawEntry> {
    let mut parser = alt((parse_range_entry, parse_single_value_entry));
    parser(input)
}

fn parse_value_sequence(input: &str) -> IResult<&str, Vec<RawEntry>> {
    let mut parser = separated_list0(char(','), parse_entry);
    parser(input)
}

#[derive(PartialEq, Debug)]
enum RawEntry<'a> {
    SingleValue(&'a str),
    Range(RawFullRange<'a>),
}

#[derive(PartialEq, Debug)]
struct RawFullRange<'a> {
    simple_range: RawSimpleRange<'a>,
    step_size: Option<&'a str>,
}

impl<'a> RawFullRange<'a> {
    fn new(simple_range: RawSimpleRange<'a>, step_size: Option<&'a str>) -> Self {
        Self {
            simple_range,
            step_size,
        }
    }
}

#[derive(PartialEq, Debug)]
struct RawSimpleRange<'a> {
    min: &'a str,
    max: &'a str,
}

impl<'a> RawSimpleRange<'a> {
    fn new(min: &'a str, max: &'a str) -> Self {
        Self { min, max }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_size() {
        assert_eq!(parse_step_size("(0.5)"), Ok(("", "0.5")));
        assert_eq!(parse_step_size("(8)"), Ok(("", "8")));
        assert_eq!(parse_step_size("( abc )"), Ok(("", " abc ")));
    }

    #[test]
    fn simple_range() {
        assert_eq!(
            parse_simple_range("5-10"),
            Ok(("", RawSimpleRange::new("5", "10")))
        );
        assert_eq!(
            parse_simple_range("5.0 - 10.0"),
            Ok(("", RawSimpleRange::new("5.0 ", " 10.0")))
        );
        assert_eq!(
            parse_simple_range("a-f"),
            Ok(("", RawSimpleRange::new("a", "f")))
        );
    }

    #[test]
    fn full_range() {
        assert_eq!(
            parse_full_range("5-10"),
            Ok(("", RawFullRange::new(RawSimpleRange::new("5", "10"), None)))
        );
        assert_eq!(
            parse_full_range("5-10 (0.1)"),
            Ok((
                "",
                RawFullRange::new(RawSimpleRange::new("5", "10 "), Some("0.1"))
            ))
        );
    }

    #[test]
    fn entry() {
        assert_eq!(
            parse_entry("5-10 (0.1)"),
            Ok((
                "",
                RawEntry::Range(RawFullRange::new(
                    RawSimpleRange::new("5", "10 "),
                    Some("0.1")
                ))
            ))
        );
        assert_eq!(parse_entry("75.5"), Ok(("", RawEntry::SingleValue("75.5"))));
    }

    #[test]
    fn value_sequence() {
        assert_eq!(
            parse_value_sequence("5-10 (0.1), 12.5, 15 - 20"),
            Ok((
                "",
                vec![
                    RawEntry::Range(RawFullRange::new(
                        RawSimpleRange::new("5", "10 "),
                        Some("0.1")
                    )),
                    RawEntry::SingleValue(" 12.5"),
                    RawEntry::Range(RawFullRange::new(RawSimpleRange::new(" 15 ", " 20"), None))
                ]
            ))
        );
    }
}
