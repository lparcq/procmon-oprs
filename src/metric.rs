use anyhow::Result;
use libc::pid_t;
use std::collections::BTreeMap;
use thiserror::Error;

use crate::format;

#[derive(Error, Debug)]
enum Error {
    #[error("{0}: unknown metric")]
    UnknownMetric(String),
    #[error("{0}: unknown formatter")]
    UnknownFormatter(String),
}

/// Metrics that can be collected for a process
#[derive(Copy, Clone, Debug)]
pub enum MetricId {
    FaultMinor,
    FaultMajor,
    MemRss,
    MemVm,
    MemText,
    MemData,
    TimeReal,
    TimeSystem,
    TimeUser,
}

impl MetricId {
    pub fn to_str(self) -> &'static str {
        match self {
            MetricId::FaultMinor => "fault:minor",
            MetricId::FaultMajor => "fault:major",
            MetricId::MemRss => "mem:rss",
            MetricId::MemVm => "mem:vm",
            MetricId::MemText => "mem:text",
            MetricId::MemData => "mem:data",
            MetricId::TimeReal => "time:real",
            MetricId::TimeSystem => "time:system",
            MetricId::TimeUser => "time:user",
        }
    }
}

type MetricIdMap = BTreeMap<&'static str, MetricId>;

/// Mapping of metric name and id
pub struct MetricMapper {
    mapping: MetricIdMap,
}

impl MetricMapper {
    pub fn new() -> MetricMapper {
        let mut mapping = BTreeMap::new();
        mapping.insert(MetricId::FaultMinor.to_str(), MetricId::FaultMinor);
        mapping.insert(MetricId::FaultMajor.to_str(), MetricId::FaultMajor);
        mapping.insert(MetricId::MemVm.to_str(), MetricId::MemVm);
        mapping.insert(MetricId::MemRss.to_str(), MetricId::MemRss);
        mapping.insert(MetricId::MemText.to_str(), MetricId::MemText);
        mapping.insert(MetricId::MemData.to_str(), MetricId::MemData);
        mapping.insert(MetricId::TimeReal.to_str(), MetricId::TimeReal);
        mapping.insert(MetricId::TimeSystem.to_str(), MetricId::TimeSystem);
        mapping.insert(MetricId::TimeUser.to_str(), MetricId::TimeUser);
        MetricMapper { mapping }
    }

    pub fn get(&self, name: &str) -> Option<&MetricId> {
        self.mapping.get(name)
    }

    pub fn help(id: MetricId) -> &'static str {
        match id {
            MetricId::FaultMinor => "page faults without disk access",
            MetricId::FaultMajor => "page faults with disk access",
            MetricId::MemVm => "virtual memory",
            MetricId::MemRss => "resident set size",
            MetricId::MemText => "text size (code)",
            MetricId::MemData => "data + stack size",
            MetricId::TimeReal => "elapsed time since process started",
            MetricId::TimeSystem => "elapsed time in kernel mode",
            MetricId::TimeUser => "elapsed time in user mode",
        }
    }

    pub fn for_each<F>(&self, func: F)
    where
        F: Fn(MetricId, &str),
    {
        self.mapping.iter().for_each(|(name, id)| func(*id, *name));
    }
}

fn get_format(name: &str) -> std::result::Result<format::Formatter, Error> {
    match name {
        "ki" => Ok(format::kibi),
        "mi" => Ok(format::mebi),
        "gi" => Ok(format::gibi),
        "k" => Ok(format::kilo),
        "m" => Ok(format::mega),
        "g" => Ok(format::giga),
        "sz" => Ok(format::size),
        "du" => Ok(format::duration),
        _ => Err(Error::UnknownFormatter(name.to_string())),
    }
}

fn get_human_format(id: MetricId) -> format::Formatter {
    match id {
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
) -> Result<()> {
    let mapper = MetricMapper::new();
    names.iter().try_for_each(|name| {
        let tokens: Vec<&str> = name.split('/').collect();
        match mapper.get(tokens[0]) {
            Some(id) => {
                ids.push(*id);
                if tokens.len() > 1 {
                    formatters.push(get_format(tokens[1])?);
                } else if human_format {
                    formatters.push(get_human_format(*id));
                } else {
                    formatters.push(format::identity);
                }
                Ok(())
            }
            None => Err(Error::UnknownMetric(name.to_string())),
        }
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
