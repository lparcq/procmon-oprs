// Oprs -- process monitor for Linux
// Copyright (C) 2024  Laurent Pelecq
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

use getset::Getters;
use libc::pid_t;
use std::borrow::Cow;
use strum_macros::Display as StrumDisplay;

use super::{
    forest::{new_process, process_name, Process, ProcessResult},
    format, Aggregation, Collector, Forest, FormattedMetric, Limit, MetricNamesParser, ProcessInfo,
    ProcessStat, Sample, SystemConf, SystemStat, TargetContainer, TargetError, TargetId,
};

/// High-level filter on processes
#[derive(Clone, Copy, Debug, StrumDisplay)]
pub enum ProcessFilter {
    #[strum(serialize = "none")]
    None,
    #[strum(serialize = "user")]
    UserLand,
}

impl Default for ProcessFilter {
    fn default() -> Self {
        Self::UserLand
    }
}

/// Specific metrics.
pub struct ProcessMetrics<'b> {
    pub time_cpu: &'b Sample,
    pub time_elapsed: &'b Sample,
    pub mem_vm: &'b Sample,
    pub mem_rss: &'b Sample,
    pub mem_data: &'b Sample,
    pub fd_all: &'b Sample,
    pub fd_file: &'b Sample,
    pub io_read_total: &'b Sample,
    pub io_write_total: &'b Sample,
    pub thread_count: &'b Sample,
}

/// Detailled view of a process.
#[derive(Getters)]
pub struct ProcessDetails<'a> {
    #[getset(get = "pub")]
    process: Process,
    #[getset(get = "pub")]
    process_name: String,
    collector: Collector<'a>,
}

impl ProcessDetails<'_> {
    pub fn new(pid: pid_t, human: bool) -> ProcessResult<Self> {
        let metric_names = vec![
            "time:cpu-raw+ratio",
            "time:elapsed",
            "mem:vm",
            "mem:rss",
            "mem:data",
            "fd:all",
            "fd:file",
            "io:read:total",
            "io:write:total",
            "thread:count",
        ];
        let mut parser = MetricNamesParser::new(human);
        let metrics = parser.parse(&metric_names).unwrap();
        let process = new_process(pid)?;
        let process_name = process_name(&process);
        let collector = Collector::new(Cow::Owned(metrics));
        Ok(Self {
            process,
            process_name,
            collector,
        })
    }

    pub fn refresh(&mut self, system_conf: &SystemConf) {
        let proc_stat = ProcessStat::new(&self.process, system_conf);
        self.collector.collect(&self.process_name, proc_stat);
    }

    pub fn metrics(&self) -> Option<ProcessMetrics> {
        self.collector.lines().take(1).next().map(|s| {
            let samples = s.samples_as_slice();
            ProcessMetrics {
                time_cpu: &samples[0],
                time_elapsed: &samples[1],
                mem_vm: &samples[2],
                mem_rss: &samples[3],
                mem_data: &samples[4],
                fd_all: &samples[5],
                fd_file: &samples[6],
                io_read_total: &samples[7],
                io_write_total: &samples[8],
                thread_count: &samples[9],
            }
        })
    }
}

/// A process manager must define which processes must be followed.
pub trait ProcessManager {
    fn set_filter(&mut self, _filter: ProcessFilter) {}

    fn refresh(&mut self, collector: &mut Collector) -> ProcessResult<bool>;
}

/// A Process manager that process a fixed list of targets.
pub struct FlatProcessManager<'s> {
    targets: TargetContainer<'s>,
}

impl<'s> FlatProcessManager<'s> {
    pub fn new(
        system_conf: &'s SystemConf,
        metrics: &[FormattedMetric],
        target_ids: &[TargetId],
    ) -> Result<Self, TargetError> {
        let with_system = metrics
            .iter()
            .any(|metric| metric.aggregations.has(Aggregation::Ratio));

        let mut targets = TargetContainer::new(system_conf, with_system);
        targets.push_all(target_ids)?;
        targets.initialize(metrics.len());
        Ok(Self { targets })
    }

    /// Create a process manager only from PIDS. Discard PIDS that are not valid.
    pub fn with_pids(
        system_conf: &'s SystemConf,
        metrics: &[FormattedMetric],
        pids: &[pid_t],
    ) -> Self {
        let mut targets = TargetContainer::new(system_conf, true);
        pids.iter().for_each(|pid| {
            if let Err(err) = targets.push_by_pid(&TargetId::Pid(*pid)) {
                log::warn!("{pid}: {err}");
            }
        });
        targets.initialize(metrics.len());
        Self { targets }
    }
}

impl ProcessManager for FlatProcessManager<'_> {
    fn refresh(&mut self, collector: &mut Collector) -> ProcessResult<bool> {
        let targets_updated = self.targets.refresh();
        self.targets.collect(collector);
        Ok(targets_updated)
    }
}

/// A Process explorer that interactively displays the process tree.
pub struct ForestProcessManager<'s> {
    system_conf: &'s SystemConf,
    system_limits: Vec<Option<Limit>>,
    forest: Forest,
    filter: ProcessFilter,
}

impl<'s> ForestProcessManager<'s> {
    pub fn new(
        system_conf: &'s SystemConf,
        metrics: &[FormattedMetric],
    ) -> Result<Self, TargetError> {
        Ok(Self {
            system_conf,
            system_limits: vec![None; metrics.len()],
            forest: Forest::new(),
            filter: ProcessFilter::default(),
        })
    }

    fn no_filter(_pi: &ProcessInfo) -> bool {
        true
    }

    fn filter_user_land(pi: &ProcessInfo) -> bool {
        !pi.is_kernel()
    }
}

impl ProcessManager for ForestProcessManager<'_> {
    fn set_filter(&mut self, filter: ProcessFilter) {
        self.filter = filter;
    }

    fn refresh(&mut self, collector: &mut Collector) -> ProcessResult<bool> {
        let mut system = SystemStat::new(self.system_conf);
        let system_info = format!(
            "[{} cores -- {}]",
            SystemStat::num_cores().unwrap_or(0),
            SystemStat::mem_total()
                .map(format::size)
                .unwrap_or("?".to_string())
        );
        collector.rewind();
        collector.collect_system(&mut system);
        collector.record(
            &system_info,
            0,
            None,
            &system.extract_metrics(collector.metrics()),
            &self.system_limits,
        );
        let changed = self.forest.refresh_if(match self.filter {
            ProcessFilter::None => ForestProcessManager::no_filter,
            ProcessFilter::UserLand => ForestProcessManager::filter_user_land,
        })?;
        for root_pid in self.forest.root_pids() {
            self.forest
                .descendants(root_pid)?
                .filter(|pinfo| !pinfo.hidden())
                .for_each(|pinfo| {
                    let proc_stat = ProcessStat::with_parent_pid(
                        pinfo.process(),
                        pinfo.parent_pid(),
                        self.system_conf,
                    );
                    collector.collect(pinfo.name(), proc_stat);
                });
        }
        Ok(changed)
    }
}
