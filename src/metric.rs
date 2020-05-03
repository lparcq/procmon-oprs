use libc::pid_t;
use std::result;
use std::str::FromStr;
use strum::IntoEnumIterator;
use strum_macros::{EnumIter, EnumMessage, EnumString, IntoStaticStr};
use thiserror::Error;

use crate::format::{self, Formatter};

const SHORT_NAME_MAX_LEN: usize = 10;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}: unknown metric")]
    UnknownMetric(String),
    #[error("{0}: invalid metric pattern")]
    InvalidMetricPattern(String),
    #[error("{0}: unknown formatter")]
    UnknownFormatter(String),
}

/// Metrics that can be collected for a process
#[derive(Copy, Clone, Debug, PartialEq, EnumIter, EnumString, EnumMessage, IntoStaticStr)]
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
        serialize = "time:real",
        message = "elapsed time since process started"
    )]
    TimeReal,
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
            MetricId::TimeReal => Some("tm:real"),
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

/// Metric names parser
pub struct MetricNamesParser {
    metric_ids: Vec<MetricId>,
    formatters: Vec<Formatter>,
    human_format: bool,
}

impl MetricNamesParser {
    pub fn new(human_format: bool) -> MetricNamesParser {
        MetricNamesParser {
            metric_ids: Vec::new(),
            formatters: Vec::new(),
            human_format,
        }
    }

    // Convert unit name to a formatter
    fn get_format(name: &str) -> std::result::Result<Formatter, Error> {
        match name {
            "ki" => Ok(format::kibi),
            "mi" => Ok(format::mebi),
            "gi" => Ok(format::gibi),
            "ti" => Ok(format::tebi),
            "k" => Ok(format::kilo),
            "m" => Ok(format::mega),
            "g" => Ok(format::giga),
            "t" => Ok(format::tera),
            "sz" => Ok(format::size),
            "du" => Ok(format::duration_human),
            _ => Err(Error::UnknownFormatter(name.to_string())),
        }
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
            MetricId::TimeReal => format::duration_human,
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
                MetricId::TimeReal | MetricId::TimeSystem | MetricId::TimeUser => {
                    format::duration_seconds
                }
                _ => format::identity,
            }
        }
    }

    /// Expands limited globbing
    /// Allowed: prefix mem:*, suffix *:call, middle io:*:call
    fn expand_metric_name(
        &mut self,
        name: &str,
        fmt: Option<Formatter>,
    ) -> result::Result<(), Error> {
        let matches: Vec<MetricId> = if name.starts_with("*:") {
            // match by suffix
            let suffix = &name[2..];
            MetricId::iter()
                .filter(|id| id.to_str().ends_with(suffix))
                .collect()
        } else if name.ends_with(":*") {
            // match by prefix
            let prefix = &name[..name.len() - 2];
            MetricId::iter()
                .filter(|id| id.to_str().starts_with(prefix))
                .collect()
        } else {
            let parts: Vec<&str> = name.split(":*:").collect();
            if parts.len() != 2 {
                return Err(Error::InvalidMetricPattern(String::from(name)));
            }
            let prefix = parts[0];
            let suffix = parts[1];
            MetricId::iter()
                .filter(|id| {
                    let name = id.to_str();
                    name.starts_with(prefix) && name.ends_with(suffix)
                })
                .collect()
        };
        if matches.is_empty() {
            Err(Error::UnknownMetric(name.to_string()))
        } else {
            matches.iter().for_each(|id| {
                self.metric_ids.push(*id);
                self.formatters
                    .push(fmt.unwrap_or_else(|| self.get_default_formatter(*id)));
            });
            Ok(())
        }
    }

    /// Return a list of ids from name
    pub fn parse_metric_names(&mut self, names: &[String]) -> result::Result<(), Error> {
        names.iter().try_for_each(|name| {
            let tokens: Vec<&str> = name.split('/').collect();
            let fmt = if tokens.len() > 1 {
                Some(MetricNamesParser::get_format(tokens[1])?)
            } else {
                None
            };
            match MetricId::from_str(tokens[0]) {
                Ok(id) => {
                    self.metric_ids.push(id);
                    self.formatters
                        .push(fmt.unwrap_or_else(|| self.get_default_formatter(id)));
                }
                Err(_) => {
                    self.expand_metric_name(tokens[0], fmt)?;
                }
            }
            Ok(())
        })?;
        Ok(())
    }

    pub fn get_metric_ids(&self) -> &Vec<MetricId> {
        &self.metric_ids
    }

    pub fn get_formatters(&self) -> &Vec<Formatter> {
        &self.formatters
    }
}

/// List of values collected
pub type MetricSeries = Vec<u64>;

/// Process metrics inclued the process id and the list of metrics
pub struct ProcessMetrics {
    pub pid: pid_t,
    pub series: MetricSeries,
}

impl ProcessMetrics {
    pub fn new(pid: pid_t, series: MetricSeries) -> ProcessMetrics {
        ProcessMetrics { pid, series }
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
            "time:real/du",
            "time:system",
            "time:user",
            "thread:count",
        ]);
        // Check few metrics
        let mut parser1 = MetricNamesParser::new(false);
        parser1.parse_metric_names(&metric_names[0..2]).unwrap();
        assert_eq!(2, parser1.get_metric_ids().len());
        assert_eq!(2, parser1.get_formatters().len());

        // Check all metrics
        let mut parser2 = MetricNamesParser::new(false);
        let metric_count = metric_names.len();
        parser2.parse_metric_names(&metric_names).unwrap();
        assert_eq!(metric_count, parser2.get_metric_ids().len());
        assert_eq!(metric_count, parser2.get_formatters().len());
    }

    #[test]
    fn test_expand_metric_names() {
        // Check prefix
        let metric_names1 = vec_of_string(&["mem:*"]);
        let mut parser1 = MetricNamesParser::new(false);
        parser1.parse_metric_names(&metric_names1).unwrap();
        let metrics1 = parser1.get_metric_ids();
        assert_eq!(4, metrics1.len());
        assert_eq!(4, parser1.get_formatters().len());

        // Check suffix
        let metric_names2 = vec_of_string(&["*:storage"]);
        let mut parser2 = MetricNamesParser::new(false);
        parser2.parse_metric_names(&metric_names2).unwrap();
        let metrics2 = parser2.get_metric_ids();
        assert_eq!(2, metrics2.len());
        assert_eq!(2, parser2.get_formatters().len());

        // Check middle
        let metric_names3 = vec_of_string(&["io:*:count"]);
        let mut parser3 = MetricNamesParser::new(false);
        parser3.parse_metric_names(&metric_names3).unwrap();
        let metrics3 = parser3.get_metric_ids();
        assert_eq!(2, metrics3.len());
        assert_eq!(2, parser3.get_formatters().len());
    }

    #[test]
    fn test_expand_metric_names_errors() {
        for pattern in &["mem:*:*", "me*", "not:*"] {
            let metric_names = vec_of_string(&[pattern]);
            let mut parser = MetricNamesParser::new(false);
            assert!(
                parser.parse_metric_names(&metric_names).is_err(),
                "pattern \"{}\" works unexpectedly",
                pattern
            );
        }
    }
}
