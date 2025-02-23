// Oprs -- process monitor for Linux
// Copyright (C) 2024-2025  Laurent Pelecq
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

use getset::{Getters, Setters};
use libc::pid_t;
use strum::Display as StrumDisplay;

#[cfg(feature = "tui")]
use std::borrow::Cow;

use super::{
    forest::{ProcessClassifier, ProcessResult},
    format, Aggregation, Collector, Forest, FormattedMetric, ProcessInfo, SystemStat,
    TargetContainer, TargetError, TargetId,
};

#[cfg(feature = "tui")]
use super::{MetricFormat, MetricNamesParser, Sample};

/// Number of idle cycles to be considered as inactive.
const INACTIVITY: u16 = 5;

/// High-level filter on processes
#[derive(Clone, Copy, Debug, StrumDisplay)]
pub enum ProcessFilter {
    #[cfg(feature = "tui")]
    #[strum(serialize = "none")]
    None,
    #[strum(serialize = "user")]
    UserLand,
    #[cfg(feature = "tui")]
    #[strum(serialize = "active")]
    Active,
}

impl Default for ProcessFilter {
    fn default() -> Self {
        Self::UserLand
    }
}

/// Context for mananagers.
#[derive(Debug, Default, Getters, Setters)]
pub struct ManagerContext {
    #[cfg(feature = "tui")]
    #[getset(set = "pub")]
    filter: ProcessFilter,
    #[getset(get_copy = "pub", set = "pub")]
    root_pid: Option<pid_t>,
}

/// Specific metrics.
#[cfg(feature = "tui")]
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
#[cfg(feature = "tui")]
#[derive(Getters)]
pub struct ProcessDetails<'a> {
    #[getset(get = "pub")]
    name: String,
    #[getset(get = "pub")]
    process: ProcessInfo,
    collector: Collector<'a>,
}

#[cfg(feature = "tui")]
impl ProcessDetails<'_> {
    pub fn new(pid: pid_t, format: MetricFormat) -> ProcessResult<Self> {
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
        let mut parser = MetricNamesParser::new(format);
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

    /// Details of the parent process.
    pub fn parent(&self) -> ProcessResult<Self> {
        let parent_pid = self.process.parent_pid();
        let process = ProcessInfo::with_pid(parent_pid)?;
        let name = process.name().to_string();
        let collector = Collector::new(Cow::Owned(
            self.collector
                .metrics()
                .cloned()
                .collect::<Vec<FormattedMetric>>(),
        ));
        Ok(Self {
            name,
            process,
            collector,
        })
    }

    /// Refresh the metrics.
    pub fn refresh(&mut self) -> ProcessResult<()> {
        self.process.refresh()?;
        self.collector.collect(&self.name, &self.process);
        Ok(())
    }

    /// Process metrics.
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
    /// Set context if supported.
    fn context(&mut self) -> Option<&mut ManagerContext> {
        None
    }

    fn refresh(&mut self, collector: &mut Collector) -> ProcessResult<bool>;
}

/// A Process manager that process a fixed list of targets.
pub struct FlatProcessManager {
    targets: TargetContainer,
}

impl FlatProcessManager {
    pub fn new(metrics: &[FormattedMetric], target_ids: &[TargetId]) -> Result<Self, TargetError> {
        let with_system = metrics
            .iter()
            .any(|metric| metric.aggregations.has(Aggregation::Ratio));

        let mut targets = TargetContainer::new(with_system);
        targets.push_all(target_ids)?;
        Ok(Self { targets })
    }

    /// Create a process manager only from PIDS. Discard PIDS that are not valid.
    #[cfg(feature = "tui")]
    pub fn with_pids(pids: &[pid_t]) -> Self {
        let mut targets = TargetContainer::new(true);
        pids.iter().for_each(|pid| {
            if let Err(err) = targets.push_by_pid(&TargetId::Pid(*pid)) {
                log::warn!("{pid}: {err}");
            }
        });
        Self { targets }
    }
}

impl ProcessManager for FlatProcessManager {
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
pub struct ForestProcessManager {
    forest: Forest,
    context: ManagerContext,
    inactivity: u16,
}

impl ForestProcessManager {
    pub fn new() -> Result<Self, TargetError> {
        Ok(Self {
            forest: Forest::new(),
            context: ManagerContext::default(),
            inactivity: 0,
        })
    }

    fn collect_descendants(
        &mut self,
        collector: &mut Collector,
        root_pids: &[pid_t],
        ignore_idleness: bool,
    ) -> ProcessResult<()> {
        for root_pid in root_pids {
            self.forest
                .descendants(*root_pid)?
                .filter(|pinfo| {
                    !pinfo.hidden() && (ignore_idleness || pinfo.idleness() < self.inactivity)
                })
                .for_each(|pinfo| collector.collect(pinfo.name(), pinfo));
        }
        Ok(())
    }
}

impl ProcessManager for ForestProcessManager {
    fn context(&mut self) -> Option<&mut ManagerContext> {
        Some(&mut self.context)
    }

    fn refresh(&mut self, collector: &mut Collector) -> ProcessResult<bool> {
        let mut system = SystemStat::new();
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
        );
        if self.inactivity < INACTIVITY {
            self.inactivity += 1;
        }
        #[cfg(feature = "tui")]
        let changed = match self.context.filter {
            ProcessFilter::None => self.forest.refresh(),
            ProcessFilter::UserLand | ProcessFilter::Active => {
                self.forest.refresh_if(&AcceptUserLand::default())
            }
        }?;
        #[cfg(not(feature = "tui"))]
        let changed = self.forest.refresh_if(&AcceptUserLand::default())?;
        #[cfg(feature = "tui")]
        let ignore_idleness = !matches!(self.context.filter, ProcessFilter::Active);
        #[cfg(not(feature = "tui"))]
        let ignore_idleness = false;
        match self.context.root_pid {
            Some(root_pid) if self.forest.has_process(root_pid) => {
                self.collect_descendants(collector, &[root_pid], ignore_idleness)?
            }
            Some(_) => (),
            None => {
                self.collect_descendants(collector, &self.forest.root_pids(), ignore_idleness)?
            }
        }
        Ok(changed)
    }
}
