// Oprs -- process monitor for Linux
// Copyright (C) 2020-2024  Laurent Pelecq
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use log::warn;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while},
    character::complete::{char, digit1},
    combinator::{all_consuming, opt},
    multi::many0,
    sequence::{pair, preceded},
    IResult,
};
use std::result;
use std::str::FromStr;
use strum::IntoEnumIterator;

use super::{
    format::{self, Formatter},
    Aggregation, AggregationSet, MetricId,
};

const KILO: usize = 1000;
const MEGA: usize = 1000 * KILO;
const GIGA: usize = 1000 * MEGA;
const TERA: usize = 1000 * GIGA;

#[derive(thiserror::Error, Debug)]
pub enum ParseError {
    #[error("syntax error")]
    SyntaxError,
    #[error("value error")]
    ValueError,
}

/// Intermediate function to parse a size into two strings.
fn parse_size_partial(input: &str) -> IResult<&str, (&str, Option<&str>)> {
    pair(digit1, opt(alt((tag("k"), tag("m"), tag("g"), tag("t")))))(input)
}

/// Parse size with optional units (ex: 5k)
pub fn parse_size(input: &str) -> result::Result<u64, ParseError> {
    let (_, (value, unit)) =
        all_consuming(parse_size_partial)(input).map_err(|_| ParseError::SyntaxError)?;
    let factor = match unit {
        None => 1,
        Some("k") => KILO,
        Some("m") => MEGA,
        Some("g") => GIGA,
        Some("t") => TERA,
        Some(_) => panic!("internal error: arm should be unreachable"),
    } as u64;
    let value = value.parse::<u64>().map_err(|_| ParseError::ValueError)?;
    Ok(value * factor)
}

/// Expands limited globbing
/// Allowed: prefix mem:*, suffix *:call, middle io:*:call
fn expand_metric_name(metric_ids: &mut Vec<MetricId>, name: &str) {
    if let Some(suffix) = name.strip_prefix("*:") {
        // match by suffix
        MetricId::iter()
            .filter(|id| id.as_str().ends_with(suffix))
            .for_each(|id| metric_ids.push(id));
    } else if let Some(prefix) = name.strip_suffix(":*") {
        // match by prefix
        MetricId::iter()
            .filter(|id| id.as_str().starts_with(prefix))
            .for_each(|id| metric_ids.push(id));
    } else {
        let parts: Vec<&str> = name.split(":*:").collect();
        if parts.len() != 2 {
            return;
        }
        let prefix = parts[0];
        let suffix = parts[1];
        MetricId::iter()
            .filter(|id| {
                let name = id.as_str();
                name.starts_with(prefix) && name.ends_with(suffix)
            })
            .for_each(|id| metric_ids.push(id));
    }
}

/// parse a metric name or pattern
fn parse_metric_pattern(input: &str) -> IResult<&str, &str> {
    take_while(|c: char| c == ':' || c == '*' || c.is_ascii_lowercase())(input)
}

/// Parse metric name such as abc:def
fn parse_metric(input: &str) -> IResult<&str, Vec<MetricId>> {
    let (input, name) = parse_metric_pattern(input)?;
    let mut metric_ids = Vec::new();
    match MetricId::from_str(name) {
        Ok(id) => metric_ids.push(id),
        Err(_) => expand_metric_name(&mut metric_ids, name),
    }
    Ok((input, metric_ids))
}

/// Parse aggregations: optional -raw followed by optional +min, ...
fn parse_aggregations(input: &str) -> IResult<&str, AggregationSet> {
    let mut agg = AggregationSet::new();
    let (input, res) = opt(preceded(char('-'), tag("raw")))(input)?;
    if res.is_none() {
        agg.set(Aggregation::None);
    }
    let (input, variants) = many0(preceded(
        char('+'),
        alt((tag("min"), tag("max"), tag("ratio"))),
    ))(input)?;
    for name in variants {
        agg.set(Aggregation::from_str(name).unwrap());
    }
    Ok((input, agg))
}

/// Parse format specification /unit (ex: /ki)
fn parse_formatter(input: &str) -> IResult<&str, Option<Formatter>> {
    let (input, res) = opt(preceded(
        char('/'),
        alt((
            tag("ki"),
            tag("mi"),
            tag("gi"),
            tag("ti"),
            tag("k"),
            tag("m"),
            tag("g"),
            tag("t"),
            tag("sz"),
            tag("du"),
        )),
    ))(input)?;
    Ok((
        input,
        res.map(|name| match name {
            "ki" => format::kibi,
            "mi" => format::mebi,
            "gi" => format::gibi,
            "ti" => format::tebi,
            "k" => format::kilo,
            "m" => format::mega,
            "g" => format::giga,
            "t" => format::tera,
            "sz" => format::size,
            "du" => format::human_milliseconds,
            _ => panic!("not reachable"),
        }),
    ))
}

/// Parse metric specification with possibly garbage at the end
fn parse_metric_spec_partial(
    input: &str,
) -> IResult<&str, (Vec<MetricId>, AggregationSet, Option<Formatter>)> {
    let (input, metric_ids) = parse_metric(input)?;
    let (input, aggs) = parse_aggregations(input)?;
    let (input, fmt) = parse_formatter(input)?;
    Ok((input, (metric_ids, aggs, fmt)))
}

/// Parse metric specification name[-raw][+modifier]*[/unit]
pub fn parse_metric_spec(
    input: &str,
) -> result::Result<(Vec<MetricId>, AggregationSet, Option<Formatter>), ()> {
    match all_consuming(parse_metric_spec_partial)(input) {
        Ok((_, res)) => Ok(res),
        Err(err) => {
            warn!("{}: parsing failed: {:?}", input, err);
            Err(())
        }
    }
}

#[cfg(test)]
mod tests {

    use super::{parse_metric_spec, parse_size, Aggregation, MetricId, ParseError};

    #[test]
    fn test_wo_raw_w_max() {
        let (metric_ids, aggs, fmt) = parse_metric_spec("mem:vm-raw+max/sz").unwrap();
        assert_eq!(&[MetricId::MemVm], metric_ids.as_slice());
        assert!(aggs.has(Aggregation::Max));
        assert!(!aggs.has(Aggregation::None));
        assert!(!aggs.has(Aggregation::Min));
        assert!(!aggs.has(Aggregation::Ratio));
        let fmt = fmt.unwrap();
        assert_eq!("1.0 K", fmt(1000));
    }

    #[test]
    fn test_w_raw_min_ratio() {
        let (metric_ids, aggs, fmt) = parse_metric_spec("io:write:call+min+ratio").unwrap();
        assert_eq!(&[MetricId::IoWriteCall], metric_ids.as_slice());
        assert!(aggs.has(Aggregation::None));
        assert!(aggs.has(Aggregation::Min));
        assert!(!aggs.has(Aggregation::Max));
        assert!(aggs.has(Aggregation::Ratio));
        assert!(fmt.is_none());
    }

    #[test]
    fn test_with_format() {
        let (metric_ids, aggs, fmt) = parse_metric_spec("mem:data/ki").unwrap();
        assert_eq!(&[MetricId::MemData], metric_ids.as_slice());
        assert!(aggs.has(Aggregation::None));
        assert!(!aggs.has(Aggregation::Min));
        assert!(!aggs.has(Aggregation::Max));
        assert!(!aggs.has(Aggregation::Ratio));
        let fmt = fmt.unwrap();
        assert_eq!("1.0 Ki", fmt(1000));
    }

    #[test]
    fn test_name_only() {
        let (metric_ids, aggs, fmt) = parse_metric_spec("fault:minor").unwrap();
        assert_eq!(&[MetricId::FaultMinor], metric_ids.as_slice());
        assert!(aggs.has(Aggregation::None));
        assert!(!aggs.has(Aggregation::Min));
        assert!(!aggs.has(Aggregation::Max));
        assert!(!aggs.has(Aggregation::Ratio));
        assert!(fmt.is_none());
    }

    #[test]
    fn test_syntax_error() {
        for name in &["fault:minor#raw", "fault:minor/km"] {
            if parse_metric_spec(name).is_ok() {
                panic!("parsing must fail: {}", name);
            }
        }
    }

    #[test]
    fn parse_sizes() -> Result<(), ParseError> {
        assert_eq!(123, parse_size("123")?);
        assert_eq!(123_000, parse_size("123k")?);
        assert_eq!(15_000_000, parse_size("15m")?);
        assert_eq!(2_000_000_000, parse_size("2g")?);
        Ok(())
    }
}
