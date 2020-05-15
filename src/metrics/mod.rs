// Oprs -- process monitor for Linux
// Copyright (C) 2020  Laurent Pelecq
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

use std::cmp;
use std::collections::BTreeSet;
use std::result;
use strum_macros::{EnumIter, EnumMessage, EnumString, IntoStaticStr};
use thiserror::Error;

use self::parser::parse_metric_spec;
use crate::agg::AggregationSet;
use crate::format::{self, Formatter};

mod parser;

const SHORT_NAME_MAX_LEN: usize = 10;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}: duplicate metric")]
    DuplicateMetric(String),
    #[error("invalid syntax: {0}")]
    InvalidSyntax(String),
    #[error("{0}: unknown metric or pattern")]
    UnknownMetric(String),
}

/// Metrics that can be collected for a process
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, PartialOrd, EnumIter, EnumString, EnumMessage, IntoStaticStr,
)]
pub enum MetricId {
    #[strum(serialize = "fault:minor", message = "page faults without disk access")]
    FaultMinor,
    #[strum(serialize = "fault:major", message = "page faults with disk access")]
    FaultMajor,
    #[strum(
        serialize = "io:read:call",
        message = "number of read operations with system calls such as read(2) and pread(2)"
    )]
    IoReadCall,
    #[strum(
        serialize = "io:read:count",
        message = "number of bytes read from storage or page cache"
    )]
    IoReadCount,
    #[strum(
        serialize = "io:read:storage",
        message = "number of bytes really fetched from storage"
    )]
    IoReadStorage,
    #[strum(
        serialize = "io:write:call",
        message = "number of write operations with system calls such as write(2) and pwrite(2)"
    )]
    IoWriteCall,
    #[strum(
        serialize = "io:write:count",
        message = "number of bytes written to storage or page cache"
    )]
    IoWriteCount,
    #[strum(
        serialize = "io:write:storage",
        message = "number of bytes really sent to storage"
    )]
    IoWriteStorage,
    #[strum(serialize = "mem:rss", message = "resident set size")]
    MemRss,
    #[strum(serialize = "mem:vm", message = "virtual memory")]
    MemVm,
    #[strum(serialize = "mem:text", message = "text size (code)")]
    MemText,
    #[strum(serialize = "mem:data", message = "data + stack size")]
    MemData,
    #[strum(
        serialize = "time:elapsed",
        message = "elapsed time since process started"
    )]
    TimeElapsed,
    #[strum(
        serialize = "time:cpu",
        message = "elapsed time in kernel or user mode"
    )]
    TimeCpu,
    #[strum(serialize = "time:system", message = "elapsed time in kernel mode")]
    TimeSystem,
    #[strum(serialize = "time:user", message = "elapsed time in user mode")]
    TimeUser,
    #[strum(serialize = "thread:count", message = "number of threads")]
    ThreadCount,
}

impl MetricId {
    pub fn to_str(self) -> &'static str {
        self.into()
    }

    /// Return a string of less than SHORT_NAME_MAX_LEN characters.
    pub fn to_short_str(self) -> Option<&'static str> {
        match self {
            MetricId::FaultMinor => Some("flt:min"),
            MetricId::FaultMajor => Some("flt:maj"),
            MetricId::IoReadCall => Some("rd:call"),
            MetricId::IoReadCount => Some("rd:cnt"),
            MetricId::IoReadStorage => Some("rd:store"),
            MetricId::IoWriteCall => Some("wr:call"),
            MetricId::IoWriteCount => Some("wr:cnt"),
            MetricId::IoWriteStorage => Some("wr:store"),
            MetricId::TimeElapsed => Some("tm:elapsed"),
            MetricId::TimeCpu => Some("tm:cpu"),
            MetricId::TimeSystem => Some("tm:sys"),
            MetricId::TimeUser => Some("tm:user"),
            MetricId::ThreadCount => Some("thread:cnt"),
            _ => {
                let name: &'static str = self.into();
                if name.len() > SHORT_NAME_MAX_LEN {
                    panic!("{}: internal error, no short name", name)
                }
                Some(name)
            }
        }
    }
}

/// Ordering for BTreeMap
impl cmp::Ord for MetricId {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        fn ordinal(id: MetricId) -> u8 {
            match id {
                MetricId::FaultMinor => 0,
                MetricId::FaultMajor => 1,
                MetricId::IoReadCall => 2,
                MetricId::IoReadCount => 3,
                MetricId::IoReadStorage => 4,
                MetricId::IoWriteCall => 5,
                MetricId::IoWriteCount => 6,
                MetricId::IoWriteStorage => 7,
                MetricId::MemRss => 8,
                MetricId::MemVm => 9,
                MetricId::MemText => 10,
                MetricId::MemData => 11,
                MetricId::TimeElapsed => 12,
                MetricId::TimeCpu => 13,
                MetricId::TimeSystem => 14,
                MetricId::TimeUser => 15,
                MetricId::ThreadCount => 16,
            }
        }
        ordinal(*self).cmp(&ordinal(*other))
    }
}

/// Metric with associated aggregations and a formatter function
pub struct FormattedMetric {
    pub id: MetricId,
    pub aggregations: AggregationSet,
    pub format: Formatter,
}

impl FormattedMetric {
    fn new(id: MetricId, aggregations: AggregationSet, format: Formatter) -> FormattedMetric {
        FormattedMetric {
            id,
            aggregations,
            format,
        }
    }
}

/// Metric names parser
pub struct MetricNamesParser {
    human_format: bool,
}

impl MetricNamesParser {
    pub fn new(human_format: bool) -> MetricNamesParser {
        MetricNamesParser { human_format }
    }

    // Return the more readable format for a human
    fn get_human_format(id: MetricId) -> Formatter {
        match id {
            MetricId::IoReadCall => format::size,
            MetricId::IoReadCount => format::size,
            MetricId::IoReadStorage => format::size,
            MetricId::IoWriteCall => format::size,
            MetricId::IoWriteCount => format::size,
            MetricId::IoWriteStorage => format::size,
            MetricId::MemRss => format::size,
            MetricId::MemVm => format::size,
            MetricId::MemText => format::size,
            MetricId::MemData => format::size,
            MetricId::TimeElapsed => format::duration_human,
            MetricId::TimeCpu => format::duration_human,
            MetricId::TimeSystem => format::duration_human,
            MetricId::TimeUser => format::duration_human,
            _ => format::identity,
        }
    }

    fn get_default_formatter(&self, id: MetricId) -> Formatter {
        if self.human_format {
            MetricNamesParser::get_human_format(id)
        } else {
            match id {
                MetricId::TimeElapsed
                | MetricId::TimeCpu
                | MetricId::TimeSystem
                | MetricId::TimeUser => format::duration_seconds,
                _ => format::identity,
            }
        }
    }

    /// Return a list of metrics with aggregations and format
    pub fn parse(&mut self, names: &[String]) -> result::Result<Vec<FormattedMetric>, Error> {
        let mut metrics = Vec::new();
        let mut parsed_ids = BTreeSet::new();
        names
            .iter()
            .try_for_each(|name| match parse_metric_spec(name.as_str()) {
                Ok((metric_ids, aggs, fmt)) => {
                    if metric_ids.is_empty() {
                        return Err(Error::UnknownMetric(name.to_string()));
                    }
                    for id in metric_ids {
                        if parsed_ids.contains(&id) {
                            return Err(Error::DuplicateMetric(id.to_str().to_string()));
                        } else {
                            parsed_ids.insert(id);
                            metrics.push(FormattedMetric::new(
                                id,
                                aggs,
                                fmt.unwrap_or_else(|| self.get_default_formatter(id)),
                            ));
                        }
                    }
                    Ok(())
                }
                Err(_) => Err(Error::InvalidSyntax(format!("{}: invalid metric", name))),
            })?;
        Ok(metrics)
    }
}

#[cfg(test)]
mod tests {

    use std::str::FromStr;
    use strum::{EnumMessage, IntoEnumIterator};

    use super::{MetricId, MetricNamesParser};

    fn vec_of_string(vstr: &[&str]) -> Vec<String> {
        vstr.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_metricid_to_str() {
        assert_eq!("mem:vm", MetricId::MemVm.to_str());
        let name: &'static str = MetricId::MemVm.into();
        assert_eq!("mem:vm", name);
    }

    #[test]
    fn test_metricid_from_str() {
        assert_eq!(MetricId::MemVm, MetricId::from_str("mem:vm").unwrap());
    }

    #[test]
    fn test_metricid_help() {
        assert_eq!("virtual memory", MetricId::MemVm.get_message().unwrap());
        for metric_id in MetricId::iter() {
            assert!(
                metric_id.get_message().is_some(),
                "{}: no message",
                metric_id.to_str()
            )
        }
    }

    #[test]
    fn test_metricid_short_str() {
        for metric_id in MetricId::iter() {
            match metric_id.to_short_str() {
                Some(name) => assert!(
                    name.len() <= super::SHORT_NAME_MAX_LEN,
                    "{}: short name is too long",
                    metric_id.to_str()
                ),
                None => panic!("{} has no short name", metric_id.to_str()),
            }
        }
    }

    #[test]
    fn test_parse_metric_names() {
        let metric_names = vec_of_string(&[
            "fault:minor",
            "fault:major/k",
            "io:read:call",
            "io:read:count/sz",
            "io:read:storage",
            "io:write:call",
            "io:write:count",
            "io:write:storage",
            "mem:rss/mi",
            "mem:vm/ti",
            "mem:text/m",
            "mem:data/g",
            "time:elapsed/du",
            "time:system",
            "time:user",
            "thread:count",
        ]);
        // Check few metrics
        let mut parser1 = MetricNamesParser::new(false);
        let metrics1 = parser1.parse(&metric_names[0..2]).unwrap();
        assert_eq!(2, metrics1.len());

        // Check all metrics
        let mut parser2 = MetricNamesParser::new(false);
        let metric_count = metric_names.len();
        let metrics2 = parser2.parse(&metric_names).unwrap();
        assert_eq!(metric_count, metrics2.len());
    }

    #[test]
    fn test_expand_metric_names() {
        // Check prefix
        let metric_names1 = vec_of_string(&["mem:*"]);
        let mut parser1 = MetricNamesParser::new(false);
        let metrics1 = parser1.parse(&metric_names1).unwrap();
        assert_eq!(4, metrics1.len());

        // Check suffix
        let metric_names2 = vec_of_string(&["*:storage"]);
        let mut parser2 = MetricNamesParser::new(false);
        let metrics2 = parser2.parse(&metric_names2).unwrap();
        assert_eq!(2, metrics2.len());

        // Check middle
        let metric_names3 = vec_of_string(&["io:*:count"]);
        let mut parser3 = MetricNamesParser::new(false);
        let metrics3 = parser3.parse(&metric_names3).unwrap();
        assert_eq!(2, metrics3.len());
    }

    #[test]
    fn test_expand_metric_names_errors() {
        for pattern in &["mem:*:*", "me*", "not:*"] {
            let metric_names = vec_of_string(&[pattern]);
            let mut parser = MetricNamesParser::new(false);
            assert!(
                parser.parse(&metric_names).is_err(),
                "pattern \"{}\" works unexpectedly",
                pattern
            );
        }
    }
}
