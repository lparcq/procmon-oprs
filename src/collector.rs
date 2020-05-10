use libc::pid_t;

use crate::metrics::{AggregationMap, MetricId, MetricSeries};

/// A line for a process in a monitor
///
/// Hold a series of unsigned integer values
pub struct ProcessLine {
    pub name: String,
    pub pid: pid_t,
    pub metrics: MetricSeries,
}

impl ProcessLine {
    fn new(name: &str, pid: pid_t, metrics: MetricSeries) -> ProcessLine {
        ProcessLine {
            name: String::from(name),
            pid,
            metrics,
        }
    }
}

/// Group of metrics and aggregated values
pub struct Group<'a> {
    line: &'a ProcessLine,
}

/// Collector
pub trait Collector {
    fn clear(&mut self);
    fn collect(&mut self, target_name: &str, pid: pid_t, values: MetricSeries);
    fn lines(&self) -> &Vec<ProcessLine>;
    fn metric_ids(&self) -> &Vec<MetricId>;
}

/// Collect a grid of metrics by process
pub struct GridCollector<'a> {
    ids: Vec<MetricId>,
    aggregations: &'a AggregationMap,
    lines: Vec<ProcessLine>,
}

impl<'a> GridCollector<'a> {
    pub fn new(
        number_of_targets: usize,
        ids: Vec<MetricId>,
        aggregations: &'a AggregationMap,
    ) -> GridCollector {
        GridCollector {
            ids,
            aggregations,
            lines: Vec::with_capacity(number_of_targets),
        }
    }
}

impl<'a> Collector for GridCollector<'a> {
    /// Clear the lines
    fn clear(&mut self) {
        self.lines = Vec::with_capacity(self.lines.capacity());
    }

    fn collect(&mut self, target_name: &str, pid: pid_t, values: MetricSeries) {
        self.lines.push(ProcessLine::new(target_name, pid, values));
    }

    fn metric_ids(&self) -> &Vec<MetricId> {
        &self.ids
    }

    /// Return lines
    fn lines(&self) -> &Vec<ProcessLine> {
        &self.lines
    }
}
