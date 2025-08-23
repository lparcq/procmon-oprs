// Oprs -- process monitor for Linux
// Copyright (C) 2020-2025  Laurent Pelecq
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

use getset::{CopyGetters, Getters};
use itertools::izip;
use libc::pid_t;
use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, VecDeque},
    slice::Iter as SliceIter,
};
use strum::IntoEnumIterator;

use super::{Aggregation, FormattedMetric, MetricId, ProcessInfo, SystemStat, format};

/// Tell if it makes sense to track metric changes
///
/// Some metrics always change or almost always change. It's better not to track them.
fn track_change(id: MetricId) -> bool {
    !matches!(
        id,
        MetricId::TimeElapsed | MetricId::TimeCpu | MetricId::TimeSystem | MetricId::TimeUser
    )
}

/// The raw sample value and the derived aggregations.
///
/// The first value in _values_ is the raw value from the system. The following
/// are the aggregations if any (min, max, ...).
///
/// Strings are the formatted values. If the samples don't contain the raw value
/// (i.e. Aggregation::None is not selected), the first element in _values_ is the
/// raw value that doesn't have a counterpart in _strings_.
#[derive(Debug, Default)]
pub struct Sample {
    values: Vec<u64>,
    strings: Vec<String>,
    trends: Vec<Ordering>,
}

impl Sample {
    fn get_raw_value(&self) -> u64 {
        self.values[0]
    }

    /// Return the numeric values.
    pub fn values(&self) -> SliceIter<'_, u64> {
        self.values.iter()
    }

    /// Return the formatted strings
    pub fn strings(&self) -> SliceIter<'_, String> {
        self.strings.iter()
    }

    /// Return the trend of formatted strings
    #[cfg(feature = "tui")]
    pub fn trends(&self) -> SliceIter<'_, Ordering> {
        self.trends.iter()
    }

    fn push_raw(&mut self, value: u64) {
        assert!(self.values.is_empty());
        self.values.push(value);
    }

    fn push(&mut self, metric: &FormattedMetric, ag: Aggregation, value: u64) {
        self.values.push(value);
        self.strings.push(match ag {
            Aggregation::Ratio => format::ratio(value),
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
                Aggregation::Ratio => format::ratio(value),
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
        }
    }
}

/// Process identity with a name and a PID
pub trait ProcessIdentity {
    fn name(&self) -> &str;
    fn pid(&self) -> pid_t;
    fn parent_pid(&self) -> Option<pid_t>;
}

/// A list of computed samples for a process
#[derive(Debug, Getters, CopyGetters)]
pub struct ProcessSamples {
    name: String,
    pid: pid_t,
    parent_pid: Option<pid_t>,
    #[getset(get_copy = "pub")]
    state: char,
    samples: Vec<Sample>,
}

impl ProcessSamples {
    fn new(
        name: &str,
        pid: pid_t,
        parent_pid: Option<pid_t>,
        state: char,
        samples: Vec<Sample>,
    ) -> ProcessSamples {
        ProcessSamples {
            name: name.to_string(),
            pid,
            parent_pid,
            state,
            samples,
        }
    }

    pub fn samples(&self) -> SliceIter<'_, Sample> {
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

impl ProcessIdentity for &ProcessSamples {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn pid(&self) -> pid_t {
        self.pid
    }

    fn parent_pid(&self) -> Option<pid_t> {
        self.parent_pid
    }
}

#[cfg(test)]
impl From<&[Vec<&str>]> for ProcessSamples {
    fn from(samples: &[Vec<&str>]) -> ProcessSamples {
        ProcessSamples {
            name: String::new(),
            pid: 0,
            state: ' ',
            parent_pid: None,
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
        pinfo: Option<&ProcessInfo>,
        metrics: &[FormattedMetric],
        values: &[u64],
    ) -> ProcessSamples {
        let pid = pinfo.map(|pi| pi.pid()).unwrap_or(0);
        let parent_pid = pinfo.map(|pi| pi.parent_pid());
        let state = pinfo.map(|pi| pi.state()).unwrap_or(' ');
        let samples = metrics
            .iter()
            .zip(values.iter())
            .map(|(metric, value_ref)| {
                let mut sample = Sample::default();
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
        ProcessSamples::new(target_name, pid, parent_pid, state, samples)
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
                        if system_delta == 0 {
                            0
                        } else {
                            delta * PERCENT_FACTOR / system_delta
                        }
                    } else {
                        log::warn!("time value goes backward (from {old_value} to {new_value})",);
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
        if pstat.pid == 0 {
            self.push_samples(pstat.samples_as_slice()); // new system values
        }
    }
}

pub struct LineIter<'b> {
    iter: SliceIter<'b, pid_t>,
    samples: &'b BTreeMap<pid_t, ProcessSamples>,
}

impl<'b> Iterator for LineIter<'b> {
    type Item = &'b ProcessSamples;
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|pid| {
            self.samples
                .get(pid)
                .expect("B-tree keys must be in PID list")
        })
    }
}

/// Collect raw samples from target and returns computed values
#[derive(Getters)]
pub struct Collector<'a> {
    /// List of tracked metrics.
    metrics: Cow<'a, [FormattedMetric]>,
    /// Samples for each process.
    samples: BTreeMap<pid_t, ProcessSamples>,
    /// Process IDs in insertion order. The B-tree keeps the ordering in PID order.
    pids: Vec<pid_t>,
    /// Samples updater.
    updater: Updater,
}

impl<'a> Collector<'a> {
    pub fn new(metrics: Cow<'a, [FormattedMetric]>) -> Self {
        Collector {
            metrics,
            samples: BTreeMap::new(),
            pids: Vec::new(),
            updater: Updater::new(),
        }
    }

    /// Start collecting from the beginning
    pub fn rewind(&mut self) {
        self.pids.clear();
    }

    /// Set idle system time
    pub fn collect_system(&mut self, system: &mut SystemStat) {
        self.updater.push_system_time(system.total_time());
    }

    /// Check if the process must appear before the last samples.
    ///
    /// Children of the same parent are sorted by PID.
    fn is_before_previous(&self, pinfo: &ProcessInfo) -> bool {
        self.pids
            .last()
            .map(|prev_pid| {
                let prev_samples = self
                    .samples
                    .get(prev_pid)
                    .expect("internal error: dangling PID");
                prev_samples
                    .parent_pid()
                    .map(|prev_parent_pid| {
                        // If it's the same parent, order by PID.
                        prev_parent_pid == pinfo.parent_pid() && prev_samples.pid() > pinfo.pid()
                    })
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    /// Record metrics
    pub fn record(&mut self, target_name: &str, pinfo: Option<&ProcessInfo>, values: &[u64]) {
        let pid = pinfo.map(|pi| pi.pid()).unwrap_or(0);
        let parent_pid = pinfo.map(|pi| pi.parent_pid());

        if pinfo
            .map(|pinfo| self.is_before_previous(pinfo))
            .unwrap_or(false)
        {
            self.pids.insert(self.pids.len() - 1, pid);
        } else {
            self.pids.push(pid);
        }
        match self.samples.get_mut(&pid) {
            Some(samples) => {
                samples.parent_pid = parent_pid;
                samples.state = pinfo.map(|pi| pi.state()).unwrap_or(' ');
                self.updater
                    .update_computed_values(&self.metrics, samples, values)
            }
            None => {
                if self
                    .samples
                    .insert(
                        pid,
                        self.updater
                            .new_computed_values(target_name, pinfo, &self.metrics, values),
                    )
                    .is_some()
                {
                    log::error!("{pid}: PID has been replaced");
                }
            }
        }
    }

    /// Collect metrics
    pub fn collect(&mut self, target_name: &str, pinfo: &ProcessInfo) {
        let values = pinfo.extract_metrics(self.metrics());
        self.record(target_name, Some(pinfo), &values);
    }

    /// Called when there is no more targets
    pub fn finish(&mut self) {
        let alive = BTreeSet::from_iter(self.pids.iter());
        self.samples.retain(|pid, _| alive.contains(pid));
    }

    pub fn metrics(&self) -> SliceIter<'_, FormattedMetric> {
        self.metrics.iter()
    }

    pub fn for_each_computed_metric<F>(iter: SliceIter<FormattedMetric>, mut func: F)
    where
        F: FnMut(MetricId, Aggregation),
    {
        iter.for_each(|metric| {
            Aggregation::iter()
                .filter(|ag| metric.aggregations.has(*ag))
                .for_each(|ag| func(metric.id, ag));
        });
    }

    /// Return lines
    pub fn lines(&self) -> LineIter<'_> {
        LineIter {
            iter: self.pids.iter(),
            samples: &self.samples,
        }
    }

    #[cfg(feature = "tui")]
    pub fn line_count(&self) -> usize {
        self.pids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pids.is_empty()
    }
}
