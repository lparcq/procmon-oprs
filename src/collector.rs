use procfs::process::Process;

use crate::metric::{MetricId, ProcessMetrics};

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
