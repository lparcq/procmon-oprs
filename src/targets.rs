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

use libc::pid_t;
use procfs::process::Process;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::result;

use crate::{
    collector::Collector,
    info::{Limit, ProcessInfo, SystemConf, SystemInfo},
    metrics::MetricDataType,
    proc_dir::{PidFinder, ProcessDir},
    utils::*,
};

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error("{0}: invalid process id")]
    InvalidProcessId(pid_t),
}

/// Different way of identifying processes
#[derive(Debug)]
pub enum TargetId {
    Pid(pid_t),
    PidFile(PathBuf),
    ProcessName(String),
    ProcessGroup(String),
    System,
}

/// Target process
trait Target {
    fn name(&self) -> &str;
    fn initialize(&mut self, collector: &Collector);
    fn collect(&self, collector: &mut Collector);
}

/// The system itself
struct SystemTarget<'a> {
    system_conf: &'a SystemConf,
    limits: Vec<Option<Limit>>,
}

impl<'a> SystemTarget<'a> {
    fn new(system_conf: &'a SystemConf) -> anyhow::Result<SystemTarget<'a>> {
        let limits = Vec::new();
        Ok(SystemTarget {
            system_conf,
            limits,
        })
    }
}

impl<'a> Target for SystemTarget<'a> {
    fn name(&self) -> &str {
        "system"
    }

    fn initialize(&mut self, collector: &Collector) {
        self.limits = vec![None; collector.metrics().len()];
    }

    fn collect(&self, collector: &mut Collector) {
        let mut system = SystemInfo::new(self.system_conf);
        collector.collect_system(&mut system);
        collector.collect(
            self.name(),
            0,
            None,
            &system.extract_metrics(collector.metrics()),
            &self.limits,
        );
    }
}

/// Target that holds a single process
trait SingleTarget: Target {
    fn refresh(&mut self) -> bool;
}

/// Process defined by a pid.
///
/// Once the process is gone, the target returns no metrics.
struct StaticTarget<'a> {
    name: String,
    proc_dir: PathBuf,
    process: Option<Process>,
    system_conf: &'a SystemConf,
}

impl<'a> StaticTarget<'a> {
    fn new(pid: pid_t, system_conf: &'a SystemConf) -> StaticTarget<'a> {
        let proc_path = ProcessDir::path(pid);
        let proc_dir = ProcessDir::new(proc_path.as_path());
        StaticTarget {
            name: proc_dir
                .process_name()
                .unwrap_or_else(|| ProcessDir::process_name_from_pid(pid)),
            proc_dir: proc_path,
            process: Process::new(pid).ok(),
            system_conf,
        }
    }

    fn new_existing(
        pid: pid_t,
        system_conf: &'a SystemConf,
    ) -> result::Result<StaticTarget<'a>, Error> {
        let target = StaticTarget::new(pid, system_conf);
        if target.has_process() {
            Ok(target)
        } else {
            Err(Error::InvalidProcessId(pid))
        }
    }

    fn has_process(&self) -> bool {
        self.process.is_some()
    }

    fn is_alive(&self) -> bool {
        self.proc_dir.exists()
    }

    fn pid(&self) -> Option<pid_t> {
        self.process.as_ref().map(|proc| proc.pid())
    }

    fn process_info(&self) -> Option<ProcessInfo> {
        self.process
            .as_ref()
            .map(|process| ProcessInfo::new(process, self.system_conf))
    }

    fn collect_with_name(&self, name: &str, collector: &mut Collector) {
        if let Some(ref process) = self.process {
            let mut proc_info = ProcessInfo::new(process, self.system_conf);
            collector.collect(
                name,
                process.pid(),
                None,
                &proc_info.extract_metrics(collector.metrics()),
                &proc_info.extract_limits(collector.metrics()),
            )
        }
    }
}

impl<'a> Target for StaticTarget<'a> {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn initialize(&mut self, _: &Collector) {}

    fn collect(&self, collector: &mut Collector) {
        self.collect_with_name(self.name(), collector);
    }
}

impl<'a> SingleTarget for StaticTarget<'a> {
    fn refresh(&mut self) -> bool {
        if self.has_process() && !self.is_alive() {
            self.process = None;
            return true;
        }
        false
    }
}

/// Process defined by a pid file.
///
/// The pid can change over the time.
struct DynamicTarget<'a> {
    name: Option<String>,
    target: Option<StaticTarget<'a>>,
    pid_file: PathBuf,
    system_conf: &'a SystemConf,
}

impl<'a> DynamicTarget<'a> {
    fn new(pid_file: &Path, system_conf: &'a SystemConf) -> DynamicTarget<'a> {
        DynamicTarget {
            name: basename(pid_file, true),
            target: read_pid_file(pid_file)
                .map_or(None, |pid| Some(StaticTarget::new(pid, system_conf))),
            pid_file: pid_file.to_path_buf(),
            system_conf,
        }
    }
}

impl<'a> Target for DynamicTarget<'a> {
    fn name(&self) -> &str {
        match &self.name {
            Some(name) => name.as_str(),
            None => match &self.target {
                Some(target) => target.name(),
                None => "<unknown>",
            },
        }
    }

    fn initialize(&mut self, _: &Collector) {}

    fn collect(&self, collector: &mut Collector) {
        if let Some(target) = &self.target {
            target.collect_with_name(self.name(), collector);
        }
    }
}

impl<'a> SingleTarget for DynamicTarget<'a> {
    fn refresh(&mut self) -> bool {
        match &mut self.target {
            Some(target) => target.refresh(),
            None => {
                if let Ok(pid) = read_pid_file(self.pid_file.as_path()) {
                    self.target = Some(StaticTarget::new(pid, self.system_conf));
                    true
                } else {
                    false
                }
            }
        }
    }
}

/// Manage a set of static targets
struct TargetSet<'a> {
    set: Vec<StaticTarget<'a>>,
    system_conf: &'a SystemConf,
}

impl<'a> TargetSet<'a> {
    fn new(system_conf: &'a SystemConf) -> TargetSet<'a> {
        TargetSet {
            set: Vec::new(),
            system_conf,
        }
    }

    fn len(&self) -> usize {
        self.set.len()
    }

    fn refresh(&mut self, current_pids: &[pid_t]) -> bool {
        let mut previous_pids = BTreeSet::new();
        let mut updated = false;
        self.set.retain(|target| {
            if let Some(pid) = target.pid() {
                previous_pids.insert(pid);
                let alive = target.is_alive();
                if !alive {
                    updated = true;
                }
                alive
            } else {
                updated = true;
                false
            }
        });
        for pid in current_pids {
            if !previous_pids.contains(pid) {
                let target = StaticTarget::new(*pid, self.system_conf);
                if target.has_process() {
                    self.set.push(target);
                    updated = true;
                }
            }
        }
        updated
    }
}

/// Container of static targets.
trait MultiTargets<'a>: Target {
    fn targets_mut<'t>(&'t mut self) -> &'t mut TargetSet<'a>;
}

/// Distinct processes with the same name.
///
/// Metrics are collected separately for each process.
struct DistinctTargets<'a> {
    name: String,
    targets: TargetSet<'a>,
}

impl<'a> DistinctTargets<'a> {
    fn new(name: &str, system_conf: &'a SystemConf) -> DistinctTargets<'a> {
        DistinctTargets {
            name: name.to_string(),
            targets: TargetSet::new(system_conf),
        }
    }
}

impl<'a> Target for DistinctTargets<'a> {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn initialize(&mut self, _: &Collector) {}

    fn collect(&self, collector: &mut Collector) {
        for target in self.targets.set.iter() {
            target.collect(collector);
        }
    }
}

impl<'a> MultiTargets<'a> for DistinctTargets<'a> {
    fn targets_mut<'t>(&'t mut self) -> &'t mut TargetSet<'a> {
        &mut self.targets
    }
}

/// Sample cache
///
/// Dead samples are the sum of the last samples of dead processes. It's used to keep
/// the total of the counters of a process that terminated.
struct SampleCache {
    is_cached: Vec<bool>,
    last_samples: BTreeMap<pid_t, Vec<u64>>,
    dead_samples: Vec<u64>,
}

impl SampleCache {
    fn new() -> SampleCache {
        SampleCache {
            is_cached: Vec::new(),
            last_samples: BTreeMap::new(),
            dead_samples: Vec::new(),
        }
    }

    fn set_cached(&mut self, is_cached: &[bool]) {
        self.is_cached = Vec::from(is_cached);
    }

    /// Fill the set with the current pids
    fn pids_set(&self, pids: &mut BTreeSet<pid_t>) {
        self.last_samples.keys().for_each(|pid| {
            let _ = pids.insert(*pid);
        });
    }

    /// Insert the last samples of a process
    fn insert(&mut self, pid: i32, samples: &[u64]) {
        self.last_samples.insert(pid, Vec::from(samples));
    }

    /// Remove a process from the last samples
    ///
    /// The last samples are added to the dead samples
    fn remove(&mut self, pid: i32) {
        if let Some(samples) = self.last_samples.remove(&pid) {
            if self.dead_samples.is_empty() {
                self.dead_samples = samples;
            } else {
                samples.iter().enumerate().for_each(|(index, value)| {
                    if self.is_cached[index] {
                        self.dead_samples[index] += value
                    }
                })
            }
        }
    }

    /// Dead samples
    fn dead_samples(&self) -> &Vec<u64> {
        &self.dead_samples
    }
}

/// Processes with the same name as a single target.
///
/// The metrics are the sum of all the process values.
///
/// As metrics of type counters are accumulated over the lifetime of the target. It's
/// necessary to remember the values of the processes that have died. Otherwise metrics
/// like the number of IO read could decrease from time to time.
struct MergedTargets<'a> {
    name: String,
    group_id: pid_t,
    targets: TargetSet<'a>,
    cache: RefCell<SampleCache>,
    limits: Vec<Option<Limit>>,
}

impl<'a> MergedTargets<'a> {
    fn new(name: &str, group_id: pid_t, system_conf: &'a SystemConf) -> MergedTargets<'a> {
        MergedTargets {
            name: name.to_string(),
            group_id,
            targets: TargetSet::new(system_conf),
            cache: RefCell::new(SampleCache::new()),
            limits: Vec::new(),
        }
    }
}

impl<'a> Target for MergedTargets<'a> {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn initialize(&mut self, collector: &Collector) {
        let are_counters = collector
            .metrics()
            .map(|metric| std::matches!(metric.id.data_type(), MetricDataType::Counter))
            .collect::<Vec<bool>>();
        self.cache.borrow_mut().set_cached(&are_counters);
        self.limits = vec![None; collector.metrics().len()];
    }

    fn collect(&self, collector: &mut Collector) {
        let mut merged_samples: Vec<u64> = collector.metrics().map(|_| 0_u64).collect();
        let mut cache = self.cache.borrow_mut();
        let mut old_pids = BTreeSet::new();
        cache.pids_set(&mut old_pids);
        for target in self.targets.set.iter() {
            if let Some(mut proc_info) = target.process_info() {
                let samples = proc_info.extract_metrics(collector.metrics());
                samples
                    .iter()
                    .enumerate()
                    .for_each(|(index, value)| merged_samples[index] += value);
                let pid = proc_info.pid();
                cache.insert(pid, &samples);
                old_pids.remove(&pid);
            }
        }
        old_pids.iter().for_each(|pid| cache.remove(*pid));
        cache
            .dead_samples()
            .iter()
            .enumerate()
            .for_each(|(index, value)| {
                merged_samples[index] += value;
            });
        collector.collect(
            self.name(),
            self.group_id,
            Some(self.targets.len()),
            &merged_samples,
            &self.limits,
        )
    }
}

impl<'a> MultiTargets<'a> for MergedTargets<'a> {
    fn targets_mut<'t>(&'t mut self) -> &'t mut TargetSet<'a> {
        &mut self.targets
    }
}

/// Target container
pub struct TargetContainer<'a> {
    system: Option<SystemTarget<'a>>,
    singles: Vec<Box<dyn SingleTarget + 'a>>,
    multis: Vec<Box<dyn MultiTargets<'a> + 'a>>,
    system_conf: &'a SystemConf,
    current_group_id: pid_t,
}

impl<'a> TargetContainer<'a> {
    pub fn new(system_conf: &'a SystemConf) -> TargetContainer<'a> {
        TargetContainer {
            system: None,
            singles: Vec::new(),
            multis: Vec::new(),
            system_conf,
            current_group_id: system_conf.max_pid(),
        }
    }

    pub fn has_system(&self) -> bool {
        self.system.is_some()
    }

    pub fn refresh(&mut self) -> bool {
        let mut changed = false;
        self.singles.iter_mut().for_each(|target| {
            if target.refresh() {
                changed = true;
            }
        });

        if !self.multis.is_empty() {
            let mut pids = Vec::new();
            {
                let mut pid_finder = PidFinder::new(&mut pids);
                self.multis
                    .iter()
                    .for_each(|target| pid_finder.register(target.name()));
                pid_finder.fill();
            }
            self.multis
                .iter_mut()
                .enumerate()
                .for_each(|(index, target)| {
                    if target.targets_mut().refresh(pids[index].as_slice()) {
                        changed = true;
                    }
                });
        }
        changed
    }

    pub fn initialize(&mut self, collector: &Collector) {
        if let Some(system) = &mut self.system {
            system.initialize(collector);
        }
        self.singles
            .iter_mut()
            .for_each(|target| target.initialize(collector));
        self.multis
            .iter_mut()
            .for_each(|target| target.initialize(collector));
    }

    pub fn collect(&self, collector: &mut Collector) {
        collector.rewind();
        if let Some(system) = &self.system {
            system.collect(collector);
        }
        self.singles
            .iter()
            .for_each(|target| target.collect(collector));
        self.multis
            .iter()
            .for_each(|target| target.collect(collector));
        collector.finish();
    }

    pub fn push(&mut self, target_id: &TargetId) -> anyhow::Result<()> {
        match target_id {
            TargetId::System => {
                if self.system.is_none() {
                    self.system = Some(SystemTarget::new(self.system_conf)?)
                }
            }
            TargetId::Pid(pid) => self.singles.push(Box::new(StaticTarget::new_existing(
                *pid,
                self.system_conf,
            )?)),
            TargetId::PidFile(pid_file) => self
                .singles
                .push(Box::new(DynamicTarget::new(pid_file, self.system_conf))),
            TargetId::ProcessName(name) => self.multis.push(Box::new(DistinctTargets::new(
                name.as_str(),
                self.system_conf,
            ))),
            TargetId::ProcessGroup(name) => {
                self.current_group_id += 1;
                self.multis.push(Box::new(MergedTargets::new(
                    name.as_str(),
                    self.current_group_id,
                    self.system_conf,
                )));
            }
        };
        Ok(())
    }

    pub fn push_all(&mut self, target_ids: &[TargetId]) -> anyhow::Result<()> {
        for target_id in target_ids {
            self.push(target_id)?;
        }
        Ok(())
    }
}
