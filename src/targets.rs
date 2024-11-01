// Oprs -- process monitor for Linux
// Copyright (C) 2020-2024  Laurent Pelecq
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
use std::path::{Path, PathBuf};

use crate::{
    collector::Collector,
    process::{
        Forest as ProcessForest, Limit, Process, ProcessError, ProcessStat, SystemConf, SystemStat,
    },
    utils::*,
};

#[derive(thiserror::Error, Debug)]
pub enum TargetError {
    #[error("{0}: invalid process id")]
    InvalidProcessId(pid_t),
    #[error("{0}: invalid path")]
    InvalidPath(PathBuf),
    #[error("{0}")]
    ProcessError(ProcessError),
}

/// Different way of identifying processes
#[derive(Debug)]
pub enum TargetId {
    Pid(pid_t),
    PidFile(PathBuf),
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
    system_conf: &'a SystemConf,
}

impl<'a> Target<'a> {
    fn new(process: Process, system_conf: &'a SystemConf) -> Self {
        let name = crate::process::process_identifier(&process);
        Self {
            name,
            process: Some(process),
            pid_file: None,
            system_conf,
        }
    }

    fn with_pid_file<P>(pid_file: P, system_conf: &'a SystemConf) -> Result<Self, TargetError>
    where
        P: AsRef<Path>,
    {
        let pid_file = pid_file.as_ref();
        Ok(Self {
            name: basename(pid_file, true)
                .ok_or_else(|| TargetError::InvalidPath(pid_file.to_path_buf()))?,
            process: None,
            pid_file: Some(pid_file.to_path_buf()),
            system_conf,
        })
    }

    fn is_alive(&self) -> bool {
        self.process
            .as_ref()
            .map(|proc| proc.is_alive())
            .unwrap_or(false)
    }

    fn set_process(&mut self, process: Process) {
        self.process = Some(process);
    }

    fn clear_process(&mut self) -> bool {
        let changed = self.process.is_some();
        self.process = None;
        changed
    }

    fn pid_file<'b>(&'b self) -> Option<&'b PathBuf> {
        self.pid_file.as_ref()
    }

    fn collect(&self, collector: &mut Collector) {
        if let Some(ref process) = self.process {
            let mut proc_info = ProcessStat::new(process, self.system_conf);
            collector.collect(
                &self.name,
                process.pid(),
                &proc_info.extract_metrics(collector.metrics()),
                &proc_info.extract_limits(collector.metrics()),
            )
        }
    }
}

/// Target container
pub struct TargetContainer<'a> {
    targets: Vec<Box<Target<'a>>>,
    system_conf: &'a SystemConf,
    system_limits: Option<Vec<Option<Limit>>>,
    with_system: bool,
}

impl<'a> TargetContainer<'a> {
    pub fn new(system_conf: &'a SystemConf, with_system: bool) -> TargetContainer<'a> {
        TargetContainer {
            targets: Vec::new(),
            system_conf,
            system_limits: None,
            with_system: with_system,
        }
    }

    pub fn refresh(&mut self) -> bool {
        let mut changed = false;
        self.targets.iter_mut().for_each(|target| {
            if !target.is_alive() && target.clear_process() {
                changed = true;
            }
            if let Some(pid_file) = target.pid_file() {
                match read_pid_file(pid_file) {
                    Ok(pid) => {
                        if let Ok(process) = Process::new(pid) {
                            target.set_process(process);
                            changed = true;
                        }
                    }
                    Err(err) => error!("{:?}", err),
                }
            }
        });
        changed
    }

    pub fn initialize(&mut self, collector: &Collector) {
        if self.with_system {
            self.system_limits = Some(vec![None; collector.metrics().len()]);
        }
    }

    pub fn collect(&self, collector: &mut Collector) {
        collector.rewind();
        if let Some(ref limits) = self.system_limits {
            let mut system = SystemStat::new(self.system_conf);
            collector.collect_system(&mut system);
            collector.collect(
                "system",
                0,
                &system.extract_metrics(collector.metrics()),
                &limits,
            );
        }
        self.targets
            .iter()
            .for_each(|target| target.collect(collector));
        collector.finish();
    }

    pub fn push(
        &mut self,
        target_id: &TargetId,
        forest: &ProcessForest,
    ) -> Result<(), TargetError> {
        match target_id {
            TargetId::System => {
                self.with_system = true;
            }
            TargetId::ProcessName(name) => {
                forest.iter_roots().for_each(|p| {
                    if let Ok(descendants) = forest.descendants(p.pid()) {
                        descendants.for_each(|p| {
                            if let Some(other_name) = p.name() {
                                if other_name == name {
                                    if let Ok(process) = Process::new(p.pid()) {
                                        self.targets
                                            .push(Box::new(Target::new(process, self.system_conf)));
                                    }
                                }
                            }
                        })
                    }
                });
            }
            _ => {
                let target = Box::new(match target_id {
                    TargetId::Pid(pid) => Target::new(
                        Process::new(*pid).map_err(|_| TargetError::InvalidProcessId(*pid))?,
                        self.system_conf,
                    ),
                    TargetId::PidFile(pid_file) => {
                        Target::with_pid_file(pid_file, self.system_conf)?
                    }
                    _ => panic!("already matched"),
                });
                self.targets.push(target);
            }
        };
        Ok(())
    }

    pub fn push_all(&mut self, target_ids: &[TargetId]) -> Result<(), TargetError> {
        let forest = {
            let mut forest = ProcessForest::new();
            forest
                .refresh()
                .map_err(|err| TargetError::ProcessError(err))?;
            forest
        };

        for target_id in target_ids {
            self.push(target_id, &forest)?;
        }
        Ok(())
    }
}
