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

/// The raw sample value and the derived aggregations.
pub struct Sample {
    values: Vec<u64>,
    strings: Vec<String>,
}

impl Sample {
    fn new() -> Sample {
        Sample {
            values: Vec::new(),
            strings: Vec::new(),
        }
    }

    fn get_raw_value(&self) -> u64 {
        self.values[0]
    }

    pub fn strings(&self) -> Iter<String> {
        self.strings.iter()
    }

    fn push(&mut self, metric: &FormattedMetric, ag: Aggregation, value: u64) {
        self.values.push(value);
        self.strings.push(match ag {
            Aggregation::Ratio => crate::format::ratio(value),
            _ => (metric.format)(value),
        });
    }

    fn update(&mut self, metric: &FormattedMetric, index: usize, ag: Aggregation, value: u64) {
        if let Some(last_value) = self.values.get_mut(index) {
            match ag {
                Aggregation::None if value == *last_value => return,
                Aggregation::Min if value >= *last_value => return,
                Aggregation::Max if value <= *last_value => return,
                _ => (),
            }
            *last_value = value;
            self.strings[index] = match ag {
                Aggregation::Ratio => crate::format::ratio(value),
                _ => (metric.format)(value),
            };
        }
    }
}

/// A list of computed samples for a process
pub struct ProcessSamples {
    name: String,
    pid: pid_t,
    samples: Vec<Sample>,
}

impl ProcessSamples {
    fn new(name: &str, pid: pid_t, samples: Vec<Sample>) -> ProcessSamples {
        ProcessSamples {
            name: name.to_string(),
            pid,
            samples,
        }
    }

    pub fn get_name(&self) -> &str {
        self.name.as_str()
    }

    pub fn get_pid(&self) -> pid_t {
        self.pid
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
        metrics: &[FormattedMetric],
        values: &[u64],
    ) -> ProcessSamples {
        let samples = metrics
            .iter()
            .zip(values.iter())
            .map(|(metric, value)| {
                let mut sample = Sample::new();
                Aggregation::iter()
                    .filter(|ag| metric.aggregations.has(*ag))
                    .for_each(|ag| match ag {
                        Aggregation::None | Aggregation::Min | Aggregation::Max => {
                            sample.push(metric, ag, *value)
                        }
                        _ => sample.push(metric, ag, 0),
                    });
                sample
            })
            .collect::<Vec<Sample>>();
        if pid == 0 {
            self.push(&samples); // new system values
        }
        ProcessSamples::new(target_name, pid, samples)
    }

    /// New value depending on the aggregation type
    fn compute_value(
        &self,
        metric: &FormattedMetric,
        metric_index: usize,
        ag: Aggregation,
        old_value: u64,
        new_value: u64,
    ) -> u64 {
        if ag == Aggregation::Ratio {
            const PERCENT_FACTOR: u64 = 1000;
            let hlen = self.system_history.len();
            match metric.id {
                MetricId::TimeCpu | MetricId::TimeSystem | MetricId::TimeUser => {
                    if hlen >= 2 {
                        let system_delta = self.system_history[hlen - 1]
                            .get(metric_index)
                            .unwrap_or(&0)
                            - self.system_history[hlen - 2]
                                .get(metric_index)
                                .unwrap_or(&0);
                        if system_delta > 0 {
                            let delta = new_value - old_value;
                            delta * PERCENT_FACTOR / system_delta
                        } else {
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
        } else {
            new_value
        }
    }

    /// Update values for an existing process
    fn update_computed_values(
        &mut self,
        metrics: &[FormattedMetric],
        proc: &mut ProcessSamples,
        values: &[u64],
    ) {
        let mut metric_index = 0;
        for (metric, sample, value_ref) in izip!(metrics, proc.get_samples_mut(), values) {
            let old_value = sample.get_raw_value();
            for (ag_index, ag) in Aggregation::iter()
                .filter(|ag| metric.aggregations.has(*ag))
                .enumerate()
            {
                let value = self.compute_value(metric, metric_index, ag, old_value, *value_ref);
                sample.update(&metric, ag_index, ag, value);
            }
            metric_index += 1;
        }
        if proc.get_pid() == 0 {
            self.push(proc.samples_as_slice()); // new system values
        }
    }
}

/// Collect raw samples from target and returns computed values
pub struct Collector<'a> {
    metrics: &'a [FormattedMetric],
    lines: VecDeque<ProcessSamples>,
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
    pub fn collect(&mut self, target_name: &str, pid: pid_t, values: Vec<u64>) {
        let line_pos = self.last_line_pos;
        while let Some(mut line) = self.lines.get_mut(line_pos) {
            if line.get_pid() == pid {
                self.updater
                    .update_computed_values(self.metrics, &mut line, &values);
                self.last_line_pos += 1;
                return;
            }
            if line.get_name() == target_name {
                // Targets keeps the process order. The one in the list doesn't exists anymore.
                self.lines.remove(line_pos);
            } else {
                // It's a different target
                break;
            }
        }
        let line = self
            .updater
            .new_computed_values(target_name, pid, self.metrics, &values);
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
    pub fn lines(&self) -> vec_deque::Iter<ProcessSamples> {
        self.lines.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}
