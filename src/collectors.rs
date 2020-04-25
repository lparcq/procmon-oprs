use anyhow::Result;
use libc::pid_t;
use procfs::process::Process;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Error, Debug)]
enum Error {
    #[error("{0}: unknown metric")]
    UnknownMetric(String),
}

/// Metrics that can be collected for a process
#[derive(Copy, Clone)]
pub enum MetricId {
    MemRss,
    MemVm,
    TimeSystem,
    TimeUser,
}

impl MetricId {
    pub fn to_str(self) -> &'static str {
        match self {
            MetricId::MemRss => "mem:rss",
            MetricId::MemVm => "mem:vm",
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
        mapping.insert(MetricId::MemVm.to_str(), MetricId::MemVm);
        mapping.insert(MetricId::MemRss.to_str(), MetricId::MemRss);
        mapping.insert(MetricId::TimeSystem.to_str(), MetricId::TimeSystem);
        mapping.insert(MetricId::TimeUser.to_str(), MetricId::TimeUser);
        MetricMapper { mapping }
    }

    pub fn help(id: MetricId) -> &'static str {
        match id {
            MetricId::MemVm => "virtual memory",
            MetricId::MemRss => "resident set size",
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

    // Return a list of ids from name
    pub fn from_names(&self, names: &[String]) -> Result<Vec<MetricId>> {
        let mut ids = Vec::new();
        names
            .iter()
            .try_for_each(|name| match self.mapping.get(name.as_str()) {
                Some(id) => {
                    ids.push(*id);
                    Ok(())
                }
                None => Err(Error::UnknownMetric(name.to_string())),
            })?;
        Ok(ids)
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
    fn new(pid: pid_t, series: MetricSeries) -> ProcessMetrics {
        ProcessMetrics { pid, series }
    }
}

/// A line for a process in a monitor
pub struct ProcessLine {
    pub name: String,
    pub metrics: Option<ProcessMetrics>,
}

impl ProcessLine {
    fn new(name: &str, metrics: Option<ProcessMetrics>) -> ProcessLine {
        ProcessLine {
            name: String::from(name),
            metrics,
        }
    }
}

/// Collector
pub trait Collector {
    fn clear(&mut self);
    fn collect(&mut self, target_name: &str, process: Option<&Process>);
    fn lines(&self) -> &Vec<ProcessLine>;
    fn metric_names(&self) -> Vec<&'static str>;
}

/// Collect a grid of metrics by process
pub struct GridCollector {
    ids: Vec<MetricId>,
    lines: Vec<ProcessLine>,
    tps: u64,
}

impl GridCollector {
    pub fn new(number_of_targets: usize, metric_ids: Vec<MetricId>) -> GridCollector {
        GridCollector {
            ids: metric_ids,
            lines: Vec::with_capacity(number_of_targets),
            tps: procfs::ticks_per_second().unwrap() as u64,
        }
    }

    /// Extract metrics for a process
    fn extract_values(&self, process: &Process) -> Vec<u64> {
        self.ids
            .iter()
            .map(|id| match id {
                MetricId::MemVm | MetricId::MemRss | MetricId::TimeSystem | MetricId::TimeUser => {
                    let stat = process.stat(); // refresh stat
                    if let Ok(stat) = stat {
                        match id {
                            MetricId::MemVm => stat.vsize,
                            MetricId::MemRss => {
                                if stat.rss < 0 {
                                    0
                                } else {
                                    stat.rss as u64
                                }
                            }
                            MetricId::TimeSystem => stat.stime / self.tps,
                            MetricId::TimeUser => stat.utime / self.tps,
                        }
                    } else {
                        0
                    }
                }
            })
            .collect()
    }
}

impl Collector for GridCollector {
    /// Clear the lines
    fn clear(&mut self) {
        self.lines = Vec::with_capacity(self.lines.capacity());
    }

    fn collect(&mut self, target_name: &str, process: Option<&Process>) {
        self.lines.push(ProcessLine::new(
            target_name,
            match process {
                Some(process) => Some(ProcessMetrics::new(
                    process.pid(),
                    self.extract_values(process),
                )),
                None => None,
            },
        ))
    }

    /// Metric names
    fn metric_names(&self) -> Vec<&'static str> {
        self.ids.iter().map(|id| id.to_str()).collect()
    }

    /// Return lines
    fn lines(&self) -> &Vec<ProcessLine> {
        &self.lines
    }
}
