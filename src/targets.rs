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
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::result;
use thiserror::Error;

use crate::collector::Collector;
use crate::info::{ProcessInfo, SystemConf, SystemInfo};
use crate::proc_dir::{PidFinder, ProcessDir};
use crate::utils::*;

#[derive(Error, Debug)]
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
    System,
}

/// Target process
pub trait Target {
    fn get_name(&self) -> &str;
    fn collect(&self, collector: &mut Collector);
}

/// The system itself
struct SystemTarget<'a> {
    system_conf: &'a SystemConf,
}

impl<'a> SystemTarget<'a> {
    fn new(system_conf: &'a SystemConf) -> anyhow::Result<SystemTarget<'a>> {
        Ok(SystemTarget { system_conf })
    }
}

impl<'a> Target for SystemTarget<'a> {
    fn get_name(&self) -> &str {
        "system"
    }

    fn collect(&self, collector: &mut Collector) {
        let mut system = SystemInfo::new(self.system_conf);
        collector.collect(
            self.get_name(),
            0,
            system.extract_metrics(collector.metrics()),
        );
    }
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

    fn get_pid(&self) -> Option<pid_t> {
        self.process.as_ref().map(|proc| proc.pid())
    }

    fn collect_with_name(&self, name: &str, collector: &mut Collector) {
        if let Some(ref process) = self.process {
            let mut proc_info = ProcessInfo::new(process, self.system_conf);
            collector.collect(
                name,
                process.pid(),
                proc_info.extract_metrics(collector.metrics()),
            )
        }
    }

    fn refresh(&mut self) -> bool {
        if self.has_process() && !self.is_alive() {
            self.process = None;
            return true;
        }
        false
    }
}

impl<'a> Target for StaticTarget<'a> {
    fn get_name(&self) -> &str {
        self.name.as_str()
    }

    fn collect(&self, collector: &mut Collector) {
        self.collect_with_name(self.get_name(), collector);
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
    fn new(pid_file: &PathBuf, system_conf: &'a SystemConf) -> DynamicTarget<'a> {
        DynamicTarget {
            name: basename(pid_file, true),
            target: read_pid_file(pid_file.as_path())
                .map_or(None, |pid| Some(StaticTarget::new(pid, system_conf))),
            pid_file: pid_file.clone(),
            system_conf,
        }
    }

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

impl<'a> Target for DynamicTarget<'a> {
    fn get_name(&self) -> &str {
        match &self.name {
            Some(name) => name.as_str(),
            None => match &self.target {
                Some(target) => target.get_name(),
                None => "<unknown>",
            },
        }
    }

    fn collect(&self, collector: &mut Collector) {
        if let Some(target) = &self.target {
            target.collect_with_name(self.get_name(), collector);
        }
    }
}

/// Process defined by name.
///
/// There may be multiple instances. The target sums the metrics.
struct MultiTarget<'a> {
    name: String,
    targets: Vec<StaticTarget<'a>>,
    system_conf: &'a SystemConf,
}

impl<'a> MultiTarget<'a> {
    fn new(name: &str, system_conf: &'a SystemConf) -> MultiTarget<'a> {
        MultiTarget {
            name: name.to_string(),
            targets: Vec::new(),
            system_conf,
        }
    }

    fn refresh(&mut self, current_pids: &[pid_t]) -> bool {
        let mut previous_pids = BTreeSet::new();
        let mut updated = false;
        self.targets.retain(|target| {
            if let Some(pid) = target.get_pid() {
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
                    self.targets.push(target);
                    updated = true;
                }
            }
        }
        updated
    }
}

impl<'a> Target for MultiTarget<'a> {
    fn get_name(&self) -> &str {
        self.name.as_str()
    }

    fn collect(&self, collector: &mut Collector) {
        for target in self.targets.iter() {
            target.collect(collector);
        }
    }
}

/// Target container
pub struct TargetContainer<'a> {
    system: Option<SystemTarget<'a>>,
    statics: Vec<StaticTarget<'a>>,
    dynamics: Vec<DynamicTarget<'a>>,
    multis: Vec<MultiTarget<'a>>,
    system_conf: &'a SystemConf,
}

impl<'a> TargetContainer<'a> {
    pub fn new(system_conf: &'a SystemConf) -> TargetContainer<'a> {
        TargetContainer {
            system: None,
            statics: Vec::new(),
            dynamics: Vec::new(),
            multis: Vec::new(),
            system_conf,
        }
    }

    pub fn has_system(&self) -> bool {
        self.system.is_some()
    }

    pub fn refresh(&mut self) -> bool {
        let mut changed = false;
        self.statics.iter_mut().for_each(|target| {
            if target.refresh() {
                changed = true;
            }
        });
        self.dynamics.iter_mut().for_each(|target| {
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
                    .for_each(|target| pid_finder.register(target.get_name()));
                pid_finder.fill();
            }
            self.multis
                .iter_mut()
                .enumerate()
                .for_each(|(index, target)| {
                    if target.refresh(pids[index].as_slice()) {
                        changed = true;
                    }
                });
        }
        changed
    }

    pub fn collect(&self, collector: &mut Collector) {
        collector.rewind();
        if let Some(system) = &self.system {
            system.collect(collector);
        }
        self.statics
            .iter()
            .for_each(|target| target.collect(collector));
        self.dynamics
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
                    self.system = Some(SystemTarget::new(&self.system_conf)?)
                }
            }
            TargetId::Pid(pid) => self
                .statics
                .push(StaticTarget::new_existing(*pid, &self.system_conf)?),
            TargetId::PidFile(pid_file) => self
                .dynamics
                .push(DynamicTarget::new(&pid_file, &self.system_conf)),
            TargetId::ProcessName(name) => self
                .multis
                .push(MultiTarget::new(name.as_str(), &self.system_conf)),
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
