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

use crate::format;

use super::{
    Aggregation, Collector, Forest, FormattedMetric, Limit, ProcessStat, SystemConf, SystemStat,
    TargetContainer, TargetError, TargetId,
};

/// A process manager must define which processes must be followed.
pub trait ProcessManager {
    fn refresh(&mut self, collector: &mut Collector) -> anyhow::Result<bool>;
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
}

impl<'s> ProcessManager for FlatProcessManager<'s> {
    fn refresh(&mut self, collector: &mut Collector) -> anyhow::Result<bool> {
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
        })
    }
}

impl<'s> ProcessManager for ForestProcessManager<'s> {
    fn refresh(&mut self, collector: &mut Collector) -> anyhow::Result<bool> {
        let mut system = SystemStat::new(self.system_conf);
        let system_info = format!(
            "[{} cores -- {}]",
            SystemStat::num_cores().unwrap_or(0),
            SystemStat::mem_total()
                .map(format::size)
                .unwrap_or("?".to_string())
        );
        collector.collect_system(&mut system);
        collector.record(
            &system_info,
            0,
            None,
            &system.extract_metrics(collector.metrics()),
            &self.system_limits,
        );
        self.forest.refresh()?;
        for root_pid in self.forest.root_pids() {
            self.forest.descendants(root_pid)?.for_each(|proc_info| {
                let proc_stat = ProcessStat::with_parent_pid(
                    proc_info.process(),
                    proc_info.parent_pid(),
                    self.system_conf,
                );
                collector.collect(proc_info.name(), proc_stat);
            });
        }
        Ok(false)
    }
}
