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
    forest::{ProcessClassifier, ProcessResult},
    format, Aggregation, Collector, Forest, FormattedMetric, Limit, MetricNamesParser, ProcessInfo,
    Sample, SystemConf, SystemStat, TargetContainer, TargetError, TargetId,
};

/// Number of idle cycles to be considered as inactive.
const INACTIVITY: u16 = 5;

/// High-level filter on processes
#[derive(Clone, Copy, Debug, StrumDisplay)]
pub enum ProcessFilter {
    #[strum(serialize = "none")]
    None,
    #[strum(serialize = "user")]
    UserLand,
    #[strum(serialize = "active")]
    Active,
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
    name: String,
    #[getset(get = "pub")]
    process: ProcessInfo,
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
        let process = ProcessInfo::with_pid(pid)?;
        let name = process.name().to_string();
        let collector = Collector::new(Cow::Owned(metrics));
        Ok(Self {
            name,
            process,
            collector,
        })
    }

    pub fn refresh(&mut self, sysconf: &SystemConf) -> ProcessResult<()> {
        self.process.refresh()?;
        self.collector.collect(&self.name, &self.process, sysconf);
        Ok(())
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
        sysconf: &'s SystemConf,
        metrics: &[FormattedMetric],
        target_ids: &[TargetId],
    ) -> Result<Self, TargetError> {
        let with_system = metrics
            .iter()
            .any(|metric| metric.aggregations.has(Aggregation::Ratio));

        let mut targets = TargetContainer::new(sysconf, with_system);
        targets.push_all(target_ids)?;
        targets.initialize(metrics.len());
        Ok(Self { targets })
    }

    /// Create a process manager only from PIDS. Discard PIDS that are not valid.
    pub fn with_pids(sysconf: &'s SystemConf, metrics: &[FormattedMetric], pids: &[pid_t]) -> Self {
        let mut targets = TargetContainer::new(sysconf, true);
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

/// Accept all processes in userland.
#[derive(Debug, Default)]
struct AcceptUserLand(());

impl ProcessClassifier for AcceptUserLand {
    fn accept(&self, pi: &ProcessInfo) -> bool {
        !pi.is_kernel()
    }
}

/// A Process explorer that interactively displays the process tree.
pub struct ForestProcessManager<'s> {
    sysconf: &'s SystemConf,
    system_limits: Vec<Option<Limit>>,
    forest: Forest,
    filter: ProcessFilter,
    inactivity: u16,
}

impl<'s> ForestProcessManager<'s> {
    pub fn new(sysconf: &'s SystemConf, metrics: &[FormattedMetric]) -> Result<Self, TargetError> {
        Ok(Self {
            sysconf,
            system_limits: vec![None; metrics.len()],
            forest: Forest::new(),
            filter: ProcessFilter::default(),
            inactivity: 0,
        })
    }
}

impl ProcessManager for ForestProcessManager<'_> {
    fn set_filter(&mut self, filter: ProcessFilter) {
        self.filter = filter;
    }

    fn refresh(&mut self, collector: &mut Collector) -> ProcessResult<bool> {
        let mut system = SystemStat::new(self.sysconf);
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
            None,
            &system.extract_metrics(collector.metrics()),
            &self.system_limits,
        );
        if self.inactivity < INACTIVITY {
            self.inactivity += 1;
        }
        let changed = match self.filter {
            ProcessFilter::None => self.forest.refresh(),
            ProcessFilter::UserLand | ProcessFilter::Active => {
                self.forest.refresh_if(&AcceptUserLand::default())
            }
        }?;
        let ignore_idleness = !matches!(self.filter, ProcessFilter::Active);
        for root_pid in self.forest.root_pids() {
            self.forest
                .descendants(root_pid)?
                .filter(|pinfo| {
                    !pinfo.hidden() && (ignore_idleness || pinfo.idleness() < self.inactivity)
                })
                .for_each(|pinfo| collector.collect(pinfo.name(), pinfo, self.sysconf));
        }
        Ok(changed)
    }
}
