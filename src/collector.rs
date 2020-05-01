use libc::pid_t;

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
    fn no_data(&mut self, target_name: &str);
    fn collect(&mut self, target_name: &str, pid: pid_t, values: Vec<u64>);
    fn lines(&self) -> &Vec<ProcessLine>;
    fn metric_ids(&self) -> &Vec<MetricId>;
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
}

impl Collector for GridCollector {
    /// Clear the lines
    fn clear(&mut self) {
        self.lines = Vec::with_capacity(self.lines.capacity());
    }

    fn no_data(&mut self, target_name: &str) {
        self.lines.push(ProcessLine::new(target_name, None));
    }

    fn collect(&mut self, target_name: &str, pid: pid_t, values: Vec<u64>) {
        self.lines.push(ProcessLine::new(
            target_name,
            Some(ProcessMetrics::new(pid, values)),
        ));
    }

    fn metric_ids(&self) -> &Vec<MetricId> {
        &self.ids
    }

    /// Return lines
    fn lines(&self) -> &Vec<ProcessLine> {
        &self.lines
    }
}
