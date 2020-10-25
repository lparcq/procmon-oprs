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

use itertools::izip;
use libc::pid_t;
use std::collections::{vec_deque, VecDeque};
use std::slice::Iter;
use strum::IntoEnumIterator;

use crate::agg::Aggregation;
use crate::metrics::{FormattedMetric, MetricId};

/// Tell if it makes sense to track metric changes
///
/// Some metrics always change or almost always change. It's better not to track them.
fn track_change(id: MetricId) -> bool {
    match id {
        MetricId::TimeElapsed | MetricId::TimeCpu | MetricId::TimeSystem | MetricId::TimeUser => {
            false
        }
        _ => true,
    }
}

/// The raw sample value and the derived aggregations.
///
/// The first value in _values_ is the raw value from the system. The following
/// are the aggregations if any (min, max, ...).
///
/// Strings are the formatted values. If the samples don't contain the raw value
/// (i.e. Aggregation::None is not selected), the first element in _values_ is the
/// raw value that doesn't have a counterpart in _strings_.
pub struct Sample {
    values: Vec<u64>,
    strings: Vec<String>,
    changed: bool,
}

impl Sample {
    fn new() -> Sample {
        Sample {
            values: Vec::new(),
            strings: Vec::new(),
            changed: false,
        }
    }

    fn get_raw_value(&self) -> u64 {
        self.values[0]
    }

    /// Return the numeric values.
    pub fn values(&self) -> Iter<u64> {
        self.values.iter()
    }

    /// Return the formatted strings
    pub fn strings(&self) -> Iter<String> {
        self.strings.iter()
    }

    /// True if value has changed
    pub fn changed(&self) -> bool {
        self.changed
    }

    fn push_raw(&mut self, value: u64) {
        assert!(self.values.is_empty());
        self.values.push(value);
    }

    fn push(&mut self, metric: &FormattedMetric, ag: Aggregation, value: u64) {
        self.values.push(value);
        self.strings.push(match ag {
            Aggregation::Ratio => crate::format::ratio(value),
            _ => (metric.format)(value),
        });
    }

    fn update_raw(&mut self, value: u64, track_change: bool) {
        let changed = self.values[0] != value;
        if changed {
            self.values[0] = value;
        }
        if track_change {
            self.changed = changed;
        }
    }

    fn update(
        &mut self,
        metric: &FormattedMetric,
        index: usize,
        ag: Aggregation,
        value: u64,
        track_change: bool,
    ) {
        if let Some(last_value) = self.values.get_mut(index) {
            self.changed = false;
            match ag {
                Aggregation::None if value == *last_value => return,
                Aggregation::Min if value >= *last_value => return,
                Aggregation::Max if value <= *last_value => return,
                _ => {
                    if let Aggregation::None = ag {
                        if track_change {
                            self.changed = true;
                        }
                    }
                    *last_value = value;
                    let offset = self.values.len() - self.strings.len();
                    self.strings[index - offset] = match ag {
                        Aggregation::Ratio => crate::format::ratio(value),
                        _ => (metric.format)(value),
                    };
                }
            }
        }
    }
}

/// A list of computed samples for a process
pub struct TargetStatus {
    name: String,
    pid: pid_t,
    count: Option<usize>,
    samples: Vec<Sample>,
}

impl TargetStatus {
    fn new(name: &str, pid: pid_t, count: Option<usize>, samples: Vec<Sample>) -> TargetStatus {
        TargetStatus {
            name: name.to_string(),
            pid,
            count,
            samples,
        }
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn pid(&self) -> pid_t {
        self.pid
    }

    pub fn count(&self) -> Option<usize> {
        self.count
    }

    pub fn set_count(&mut self, count: Option<usize>) {
        self.count = count;
    }

    pub fn samples(&self) -> Iter<Sample> {
        self.samples.iter()
    }

    pub fn samples_as_slice(&self) -> &[Sample] {
        self.samples.as_slice()
    }

    fn get_samples_mut(&mut self) -> &mut Vec<Sample> {
        &mut self.samples
    }
}

/// Update values
///
/// Keeps the history of system values to compute ratio like CPU usage.
struct Updater {
    system_history: VecDeque<Vec<u64>>,
}

impl Updater {
    fn new() -> Updater {
        Updater {
            system_history: VecDeque::with_capacity(2),
        }
    }

    /// Remove old values and push new values
    fn push(&mut self, samples: &[Sample]) {
        while self.system_history.len() > 1 {
            let _ = self.system_history.pop_front();
        }
        self.system_history.push_back(
            samples
                .iter()
                .map(|sample| sample.get_raw_value())
                .collect(),
        );
    }

    /// Computed values for a new process
    fn new_computed_values(
        &mut self,
        target_name: &str,
        pid: pid_t,
        count: Option<usize>,
        metrics: &[FormattedMetric],
        values: &[u64],
    ) -> TargetStatus {
        let samples = metrics
            .iter()
            .zip(values.iter())
            .map(|(metric, value_ref)| {
                let mut sample = Sample::new();
                if !metric.aggregations.has(Aggregation::None) {
                    sample.push_raw(*value_ref);
                }
                Aggregation::iter()
                    .filter(|ag| metric.aggregations.has(*ag))
                    .for_each(|ag| match ag {
                        Aggregation::None | Aggregation::Min | Aggregation::Max => {
                            sample.push(metric, ag, *value_ref)
                        }
                        _ => sample.push(metric, ag, 0),
                    });
                sample
            })
            .collect::<Vec<Sample>>();
        if pid == 0 {
            self.push(&samples); // new system values
        }
        TargetStatus::new(target_name, pid, count, samples)
    }

    /// Historical metrics for the system
    fn get_history(&self, age: usize, metric_index: usize) -> u64 {
        self.system_history[self.system_history.len() - age]
            .get(metric_index)
            .copied()
            .unwrap_or(0)
    }

    /// Percentage of the value on the system total
    fn compute_ratio(
        &self,
        metric: &FormattedMetric,
        metric_index: usize,
        old_value: u64,
        new_value: u64,
    ) -> u64 {
        const PERCENT_FACTOR: u64 = 1000;
        let hlen = self.system_history.len();
        match metric.id {
            MetricId::TimeCpu | MetricId::TimeSystem | MetricId::TimeUser => {
                if hlen >= 2 {
                    let old_system_value = self.get_history(2, metric_index);
                    let new_system_value = self.get_history(1, metric_index);
                    let system_delta = new_system_value - old_system_value;
                    if new_value >= old_value {
                        let delta = new_value - old_value;
                        delta * PERCENT_FACTOR / system_delta
                    } else {
                        log::warn!(
                            "time value goes backward (from {} to {})",
                            old_value,
                            new_value,
                        );
                        0
                    }
                } else {
                    0
                }
            }
            _ if hlen >= 1 => match self.system_history[hlen - 1].get(metric_index) {
                Some(system_value) if *system_value > 0 => {
                    new_value * PERCENT_FACTOR / *system_value
                }
                _ => 0,
            },
            _ => panic!("internal error"),
        }
    }

    /// Update values for an existing process
    fn update_computed_values(
        &mut self,
        count: Option<usize>,
        metrics: &[FormattedMetric],
        pstat: &mut TargetStatus,
        values: &[u64],
    ) {
        pstat.set_count(count);
        for (metric_index, (metric, sample, value_ref)) in
            izip!(metrics, pstat.get_samples_mut(), values).enumerate()
        {
            let old_value = sample.get_raw_value();
            let new_value = *value_ref;
            let mut ag_index = 0;
            if !metric.aggregations.has(Aggregation::None) {
                sample.update_raw(new_value, track_change(metric.id));
                ag_index += 1;
            }
            for ag in Aggregation::iter().filter(|ag| metric.aggregations.has(*ag)) {
                let value = match ag {
                    Aggregation::Ratio => {
                        self.compute_ratio(metric, metric_index, old_value, new_value)
                    }
                    _ => new_value,
                };
                sample.update(&metric, ag_index, ag, value, track_change(metric.id));
                ag_index += 1;
            }
        }
        if pstat.pid() == 0 {
            self.push(pstat.samples_as_slice()); // new system values
        }
    }
}

/// Collect raw samples from target and returns computed values
pub struct Collector<'a> {
    metrics: &'a [FormattedMetric],
    lines: VecDeque<TargetStatus>,
    updater: Updater,
    last_line_pos: usize,
}

impl<'a> Collector<'a> {
    pub fn new(number_of_targets: usize, metrics: &'a [FormattedMetric]) -> Collector {
        Collector {
            metrics,
            lines: VecDeque::with_capacity(number_of_targets),
            updater: Updater::new(),
            last_line_pos: 0,
        }
    }

    /// Start collecting from the beginning
    pub fn rewind(&mut self) {
        self.last_line_pos = 0;
    }

    /// Collect a target metrics
    pub fn collect(&mut self, target_name: &str, pid: pid_t, count: Option<usize>, values: &[u64]) {
        let line_pos = self.last_line_pos;
        while let Some(mut line) = self.lines.get_mut(line_pos) {
            if line.pid() == pid {
                self.updater
                    .update_computed_values(count, self.metrics, &mut line, values);
                self.last_line_pos += 1;
                return;
            }
            if line.name() == target_name {
                // Targets keeps the process order. The one in the list doesn't exists anymore.
                self.lines.remove(line_pos);
            } else {
                // It's a different target
                break;
            }
        }
        let line = self
            .updater
            .new_computed_values(target_name, pid, count, self.metrics, &values);
        if line_pos >= self.lines.len() {
            self.lines.push_back(line);
        } else {
            self.lines.insert(line_pos, line);
        }
        self.last_line_pos += 1;
    }

    /// Called when there is no more targets
    pub fn finish(&mut self) {
        self.lines.truncate(self.last_line_pos);
    }

    pub fn metrics(&self) -> Iter<FormattedMetric> {
        self.metrics.iter()
    }

    pub fn for_each_computed_metric<F>(&self, mut func: F)
    where
        F: FnMut(MetricId, Aggregation),
    {
        self.metrics.iter().for_each(|metric| {
            Aggregation::iter()
                .filter(|ag| metric.aggregations.has(*ag))
                .for_each(|ag| func(metric.id, ag));
        });
    }

    /// Return lines
    pub fn lines(&self) -> vec_deque::Iter<TargetStatus> {
        self.lines.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}
