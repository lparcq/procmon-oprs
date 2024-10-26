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
use std::cmp::Ordering;
use std::collections::{vec_deque, VecDeque};
use std::slice::Iter;
use strum::IntoEnumIterator;

use crate::{
    agg::Aggregation,
    format::Formatter,
    metrics::{FormattedMetric, MetricId},
    process::{Limit, LimitValue, SystemStat},
};

/// Tell if it makes sense to track metric changes
///
/// Some metrics always change or almost always change. It's better not to track them.
fn track_change(id: MetricId) -> bool {
    !matches!(
        id,
        MetricId::TimeElapsed | MetricId::TimeCpu | MetricId::TimeSystem | MetricId::TimeUser
    )
}

const UNLIMITED: &str = "∞";

#[derive(Clone, Debug)]
pub enum LimitKind {
    None,
    Soft,
    Hard,
}

/// Limit formatted like the corresponding metric
#[derive(Debug)]
struct FormattedLimit {
    soft: String,
    hard: String,
}

impl FormattedLimit {
    fn new(metric: &FormattedMetric, limit: Limit) -> Self {
        let soft = FormattedLimit::limit_to_string(limit.soft_limit, metric.format);
        let hard = FormattedLimit::limit_to_string(limit.hard_limit, metric.format);
        Self { soft, hard }
    }

    fn limit_to_string(value: LimitValue, fmt: Formatter) -> String {
        match value {
            LimitValue::Unlimited => UNLIMITED.to_string(),
            LimitValue::Value(value) => fmt(value),
        }
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
    trends: Vec<Ordering>,
    limit: Option<FormattedLimit>,
}

impl Sample {
    fn new(limit: Option<FormattedLimit>) -> Sample {
        Sample {
            values: Vec::new(),
            strings: Vec::new(),
            trends: Vec::new(),
            limit,
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

    /// Return the trend of formatted strings
    pub fn trends(&self) -> Iter<Ordering> {
        self.trends.iter()
    }

    /// Return the soft limit for this metric as string
    pub fn soft_limit(&'_ self) -> Option<&'_ str> {
        self.limit.as_ref().map(|fl| fl.soft.as_str())
    }

    /// Return the hard limit for this metric as string
    pub fn hard_limit(&'_ self) -> Option<&'_ str> {
        self.limit.as_ref().map(|fl| fl.hard.as_str())
    }

    pub fn limit(&'_ self, kind: LimitKind) -> Option<&'_ str> {
        match kind {
            LimitKind::None => None,
            LimitKind::Soft => self.soft_limit(),
            LimitKind::Hard => self.hard_limit(),
        }
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
        self.trends.push(Ordering::Equal);
    }

    fn update_raw(&mut self, value: u64, track_change: bool) {
        let trend = value.cmp(&self.values[0]);
        if !matches!(trend, Ordering::Equal) {
            self.values[0] = value;
        }
        if track_change {
            self.trends[0] = trend;
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
            let value = match ag {
                Aggregation::Min if value < *last_value => value,
                Aggregation::Max if value > *last_value => value,
                _ => value,
            };
            let trend = value.cmp(last_value);
            *last_value = value;
            let offset = self.values.len() - self.strings.len();
            let index = index - offset;
            self.strings[index] = match ag {
                Aggregation::Ratio => crate::format::ratio(value),
                _ => (metric.format)(value),
            };
            if track_change {
                self.trends[index] = trend;
            }
        }
    }
}

#[cfg(test)]
impl From<&[&str]> for Sample {
    fn from(strings: &[&str]) -> Sample {
        Sample {
            values: Vec::new(),
            strings: strings.iter().map(|s| s.to_string()).collect(),
            trends: vec![Ordering::Equal; strings.len()],
            limit: None,
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

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn pid(&self) -> pid_t {
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

    #[cfg(debug_assertions)]
    fn _to_debug_string(&self) -> String {
        format!(
            "{}: [{}]",
            self.name(),
            self.samples
                .iter()
                .flat_map(|s| s.strings())
                .map(|s| s.as_str())
                .collect::<Vec<&str>>()
                .join(", ")
        )
    }
}

#[cfg(test)]
impl From<&[Vec<&str>]> for ProcessSamples {
    fn from(samples: &[Vec<&str>]) -> ProcessSamples {
        ProcessSamples {
            name: String::new(),
            pid: 0,
            samples: samples.iter().map(|s| Sample::from(s.as_slice())).collect(),
        }
    }
}

/// Update values
///
/// Keeps the history of system values to compute ratio like CPU usage.
struct Updater {
    system_values: Vec<u64>,
    total_time: VecDeque<u64>,
}

impl Updater {
    fn new() -> Updater {
        Updater {
            system_values: Vec::with_capacity(2),
            total_time: VecDeque::with_capacity(2),
        }
    }

    /// Keep current system values
    fn push_samples(&mut self, samples: &[Sample]) {
        self.system_values = samples
            .iter()
            .map(|sample| sample.get_raw_value())
            .collect();
    }

    /// Remove old values and push new values
    fn push_system_time(&mut self, milliseconds: u64) {
        while self.total_time.len() > 1 {
            let _ = self.total_time.pop_front();
        }
        self.total_time.push_back(milliseconds);
    }

    /// Computed values for a new process
    fn new_computed_values(
        &mut self,
        target_name: &str,
        pid: pid_t,
        metrics: &[FormattedMetric],
        values: &[u64],
        limits: &[Option<Limit>],
    ) -> ProcessSamples {
        let samples = metrics
            .iter()
            .zip(values.iter().zip(limits.iter()))
            .map(|(metric, (value_ref, limit_ref))| {
                let flimit = limit_ref.map(|limit| FormattedLimit::new(metric, limit));
                let mut sample = Sample::new(flimit);
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
            self.push_samples(&samples); // new system values
        }
        ProcessSamples::new(target_name, pid, samples)
    }

    /// Historical metrics for the system
    fn get_total_time(&self, age: usize) -> u64 {
        match self.total_time.get(self.total_time.len() - age) {
            Some(val_ref) => *val_ref,
            None => 0,
        }
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
        let hlen = self.total_time.len();
        match metric.id {
            MetricId::TimeCpu | MetricId::TimeSystem | MetricId::TimeUser => {
                if hlen >= 2 {
                    let system_delta = self.get_total_time(1) - self.get_total_time(2);
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
            _ => match self.system_values.get(metric_index) {
                Some(val_ref) if *val_ref > 0 => new_value * PERCENT_FACTOR / *val_ref,
                _ => 0,
            },
        }
    }

    /// Update values for an existing process
    fn update_computed_values(
        &mut self,
        metrics: &[FormattedMetric],
        pstat: &mut ProcessSamples,
        values: &[u64],
    ) {
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
                sample.update(metric, ag_index, ag, value, track_change(metric.id));
                ag_index += 1;
            }
        }
        if pstat.pid() == 0 {
            self.push_samples(pstat.samples_as_slice()); // new system values
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

    /// Set idle system time
    pub fn collect_system(&mut self, system: &mut SystemStat) {
        self.updater.push_system_time(system.total_time());
    }

    /// Collect a target metrics
    pub fn collect(
        &mut self,
        target_name: &str,
        pid: pid_t,
        values: &[u64],
        limits: &[Option<Limit>],
    ) {
        let line_pos = self.last_line_pos;
        while let Some(line) = self.lines.get_mut(line_pos) {
            if line.pid() == pid {
                self.updater
                    .update_computed_values(self.metrics, line, values);
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
            .new_computed_values(target_name, pid, self.metrics, values, limits);
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

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

#[cfg(test)]
impl<'a> From<&[Vec<Vec<&str>>]> for Collector<'a> {
    fn from(statuses: &[Vec<Vec<&str>>]) -> Collector<'a> {
        let lines = VecDeque::from(
            statuses
                .iter()
                .map(|s| ProcessSamples::from(s.as_slice()))
                .collect::<Vec<ProcessSamples>>(),
        );
        Collector {
            metrics: &[],
            lines,
            updater: Updater::new(),
            last_line_pos: 0,
        }
    }
}
