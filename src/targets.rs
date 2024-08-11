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
use log::error;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use crate::{
    collector::Collector,
    info::{Limit, ProcessInfo, SystemConf, SystemInfo},
    process::Process,
    utils::*,
};

#[derive(thiserror::Error, Debug)]
pub enum CollectorError {
    #[error("{0}: invalid process id")]
    InvalidProcessId(pid_t),
    #[error("{0}: invalid path")]
    InvalidPath(PathBuf),
}

/// Different way of identifying processes
#[derive(Debug)]
pub enum TargetId {
    Pid(pid_t),
    PidFile(PathBuf),
    ParentPid(pid_t),
    ParentPidFile(PathBuf),
    ProcessName(String),
    System,
}

/// Process defined by a pid.
///
/// Once the process is gone, the target returns no metrics.
struct Target<'a> {
    name: String,
    process: Option<Process>,
    pid_file: Option<PathBuf>,
    replaceable: bool,
    system_conf: &'a SystemConf,
}

impl<'a> Target<'a> {
    fn new(process: Process, system_conf: &'a SystemConf) -> Self {
        let name = crate::process::process_identifier(&process);
        Self {
            name,
            process: Some(process),
            pid_file: None,
            replaceable: false,
            system_conf,
        }
    }

    fn with_pid_file<P>(pid_file: P, system_conf: &'a SystemConf) -> Result<Self, CollectorError>
    where
        P: AsRef<Path>,
    {
        let pid_file = pid_file.as_ref();
        Ok(Self {
            name: basename(pid_file, true)
                .ok_or_else(|| CollectorError::InvalidPath(pid_file.to_path_buf()))?,
            process: None,
            pid_file: Some(pid_file.to_path_buf()),
            replaceable: true,
            system_conf,
        })
    }

    fn with_name(name: &str, system_conf: &'a SystemConf) -> Self {
        Self {
            name: name.to_string(),
            process: None,
            pid_file: None,
            replaceable: true,
            system_conf,
        }
    }

    fn is_alive(&self) -> bool {
        self.process
            .as_ref()
            .map(|proc| proc.is_alive())
            .unwrap_or(false)
    }

    fn is_replaceable(&self) -> bool {
        self.replaceable
    }

    fn set_process(&mut self, process: Process) {
        self.process = Some(process);
    }

    fn clear_process(&mut self) -> bool {
        let changed = self.process.is_some();
        self.process = None;
        changed
    }

    fn pid(&self) -> Option<pid_t> {
        self.process.as_ref().map(|proc| proc.pid())
    }

    fn pid_file<'b>(&'b self) -> Option<&'b PathBuf> {
        self.pid_file.as_ref()
    }

    fn process_info(&self) -> Option<ProcessInfo> {
        self.process
            .as_ref()
            .map(|process| ProcessInfo::new(process, self.system_conf))
    }

    fn collect(&self, collector: &mut Collector) {
        if let Some(ref process) = self.process {
            let mut proc_info = ProcessInfo::new(process, self.system_conf);
            collector.collect(
                &self.name,
                process.pid(),
                None,
                &proc_info.extract_metrics(collector.metrics()),
                &proc_info.extract_limits(collector.metrics()),
            )
        }
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

/// Target container
pub struct TargetContainer<'a> {
    targets: Vec<Box<Target<'a>>>,
    system_conf: &'a SystemConf,
    system_limits: Option<Vec<Option<Limit>>>,
    current_group_id: pid_t,
    with_system: bool,
}

impl<'a> TargetContainer<'a> {
    pub fn new(system_conf: &'a SystemConf) -> TargetContainer<'a> {
        TargetContainer {
            targets: Vec::new(),
            system_conf,
            system_limits: None,
            current_group_id: system_conf.max_pid(),
            with_system: false,
        }
    }

    pub fn has_system(&self) -> bool {
        self.system_limits.is_some()
    }

    pub fn refresh(&mut self) -> bool {
        //let mut changed = false;
        // let mut forest = ProcessForest::new();
        // forest.refresh(|_| false);
        // self.targets.iter_mut().for_each(|target| {
        //     if !target.is_alive() && target.clear_process() {
        //         changed = true;
        //     }
        //     if target.is_replaceable() {
        //         if let Some(pid_file) = target.pid_file() {
        //             match read_pid_file(pid_file) {
        //                 Ok(pid) => {
        //                     if let Ok(process) = Process::new(pid) {
        //                         target.set_process(process);
        //                         changed = true;
        //                     }
        //                 }
        //                 Err(err) => error!("{:?}", err),
        //             }
        //         } else if let Some(process) = process_finder.remove(&target.name) {
        //             target.set_process(process);
        //             changed = true;
        //         }
        //     }
        // });
        //changed
        false
    }

    pub fn initialize(&mut self, collector: &Collector) {
        if self.with_system {
            self.system_limits = Some(vec![None; collector.metrics().len()]);
        }
    }

    pub fn collect(&self, collector: &mut Collector) {
        collector.rewind();
        if let Some(ref limits) = self.system_limits {
            let mut system = SystemInfo::new(self.system_conf);
            collector.collect_system(&mut system);
            collector.collect(
                "system",
                0,
                None,
                &system.extract_metrics(collector.metrics()),
                &limits,
            );
        }
        self.targets
            .iter()
            .for_each(|target| target.collect(collector));
        collector.finish();
    }

    pub fn push(&mut self, target_id: &TargetId) -> Result<(), CollectorError> {
        match target_id {
            TargetId::System => {
                self.with_system = true;
            }
            _ => self.targets.push(Box::new(match target_id {
                TargetId::Pid(pid) => Target::new(
                    Process::new(*pid).map_err(|_| CollectorError::InvalidProcessId(*pid))?,
                    self.system_conf,
                ),
                TargetId::PidFile(pid_file) => Target::with_pid_file(pid_file, self.system_conf)?,
                TargetId::ProcessName(name) => Target::with_name(name, self.system_conf),
                _ => panic!("already matched"),
            })),
        };
        Ok(())
    }

    pub fn push_all(&mut self, target_ids: &[TargetId]) -> Result<(), CollectorError> {
        for target_id in target_ids {
            self.push(target_id)?;
        }
        Ok(())
    }
}
