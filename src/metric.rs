use libc::pid_t;
use std::result;
use std::str::FromStr;
use strum_macros::{EnumIter, EnumMessage, EnumString, IntoStaticStr};
use thiserror::Error;

use crate::format;

const SHORT_NAME_MAX_LEN: usize = 10;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}: unknown metric")]
    UnknownMetric(String),
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
}

impl MetricId {
    pub fn to_str(&self) -> &'static str {
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

// Convert unit name to a formatter
fn get_format(name: &str) -> std::result::Result<format::Formatter, Error> {
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
        "du" => Ok(format::duration),
        _ => Err(Error::UnknownFormatter(name.to_string())),
    }
}

// Return the more readable format for a human
fn get_human_format(id: MetricId) -> format::Formatter {
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
        MetricId::TimeReal => format::duration,
        MetricId::TimeSystem => format::duration,
        MetricId::TimeUser => format::duration,
        _ => format::identity,
    }
}

// Return a list of ids from name
pub fn parse_metric_names(
    ids: &mut Vec<MetricId>,
    formatters: &mut Vec<format::Formatter>,
    names: &[String],
    human_format: bool,
) -> result::Result<(), Error> {
    names.iter().try_for_each(|name| {
        let tokens: Vec<&str> = name.split('/').collect();
        let id =
            MetricId::from_str(tokens[0]).map_err(|_| Error::UnknownMetric(name.to_string()))?;
        ids.push(id);
        if tokens.len() > 1 {
            formatters.push(get_format(tokens[1])?);
        } else if human_format {
            formatters.push(get_human_format(id));
        } else {
            formatters.push(format::identity);
        }
        Ok(())
    })?;
    Ok(())
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

    use super::MetricId;

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
        let metric_names: Vec<String> = [
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
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        // Check few metrics
        let mut metric_ids = Vec::new();
        let mut formatters = Vec::new();
        super::parse_metric_names(&mut metric_ids, &mut formatters, &metric_names[0..2], false)
            .unwrap();
        assert_eq!(2, metric_ids.len());
        assert_eq!(2, formatters.len());

        // Check all metrics
        metric_ids.clear();
        formatters.clear();
        let metric_count = metric_names.len();
        super::parse_metric_names(&mut metric_ids, &mut formatters, &metric_names, false).unwrap();
        assert_eq!(metric_count, metric_ids.len());
        assert_eq!(metric_count, formatters.len());
    }
}
