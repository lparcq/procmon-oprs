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
    MemVm,
    MemRss,
}

impl MetricId {
    pub fn to_str(&self) -> &'static str {
        match self {
            MetricId::MemVm => "mem:vm",
            MetricId::MemRss => "mem:rss",
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
        MetricMapper { mapping }
    }

    pub fn help(id: MetricId) -> &'static str {
        match id {
            MetricId::MemVm => "process virtual memory",
            MetricId::MemRss => "process resident set size",
        }
    }

    pub fn for_each<F>(&self, func: F)
    where
        F: Fn(MetricId, &str),
    {
        self.mapping.iter().for_each(|(name, id)| func(*id, *name));
    }

    // Return a list of ids from name
    pub fn from_names(&self, names: &Vec<String>) -> Result<Vec<MetricId>> {
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
    pub target_number: usize,
    pub process_number: usize,
    pub metrics: Option<ProcessMetrics>,
}

impl ProcessLine {
    fn new(
        name: &str,
        target_number: usize,
        process_number: usize,
        metrics: Option<ProcessMetrics>,
    ) -> ProcessLine {
        ProcessLine {
            name: String::from(name),
            target_number,
            process_number,
            metrics,
        }
    }
}

/// Collector
pub trait Collector {
    fn clear(&mut self);
    fn collect(
        &mut self,
        target_number: usize,
        process_number: usize,
        target_name: &str,
        process: Option<&Process>,
    );
    fn lines(&self) -> &Vec<ProcessLine>;
    fn metric_names(&self) -> Vec<&'static str>;
}

/// Collect a grid of metrics by process
pub struct GridCollector {
    ids: Vec<MetricId>,
    lines: Vec<ProcessLine>,
}

impl GridCollector {
    pub fn new(number_of_targets: usize, metric_ids: Vec<MetricId>) -> GridCollector {
        GridCollector {
            ids: metric_ids,
            lines: Vec::with_capacity(number_of_targets),
        }
    }

    /// Extract metrics for a process
    fn extract_values(&self, process: &Process) -> Vec<u64> {
        //let tps = procfs::ticks_per_second().unwrap();
        self.ids
            .iter()
            .map(|id| match id {
                MetricId::MemVm => process.stat.vsize,
                MetricId::MemRss => {
                    if process.stat.rss < 0 {
                        0
                    } else {
                        process.stat.rss as u64
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

    fn collect(
        &mut self,
        target_number: usize,
        process_number: usize,
        target_name: &str,
        process: Option<&Process>,
    ) {
        self.lines.push(ProcessLine::new(
            target_name,
            target_number,
            process_number,
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
