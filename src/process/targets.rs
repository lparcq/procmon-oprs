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
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
};

#[cfg(not(test))]
use std::fs;

#[cfg(test)]
use super::mocks::fs;

use super::{
    Collector, Forest as ProcessForest, Limit, ProcessError, ProcessInfo, SystemConf, SystemStat,
};

#[derive(thiserror::Error, Debug)]
pub enum TargetError {
    #[error("{0}: invalid process id")]
    InvalidProcessId(pid_t),
    #[error("{0}: invalid path")]
    InvalidPath(PathBuf),
    #[error("{0}: invalid process id file")]
    InvalidPidFile(PathBuf),
    #[error("{0}")]
    ProcessError(ProcessError),
}

pub type TargetResult<T> = Result<T, TargetError>;

/// Different way of identifying processes
#[derive(Debug)]
pub enum TargetId {
    Pid(pid_t),
    PidFile(PathBuf),
    ProcessName(String),
    System,
}

/// Base name of a file with or without extension
fn basename<P>(path: P, no_extension: bool) -> Option<String>
where
    P: AsRef<Path>,
{
    let basename: Option<&std::ffi::OsStr> = if no_extension {
        path.as_ref().file_stem()
    } else {
        path.as_ref().file_name()
    };
    basename.and_then(|name| name.to_str()).map(String::from)
}

/// Read file content
fn read_file_content(filename: &Path) -> io::Result<String> {
    let mut file = fs::File::open(filename)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

/// Read a PID file and returns the PID it contains
fn read_pid_file(pid_file: &Path) -> TargetResult<pid_t> {
    read_file_content(pid_file)
        .map_err(|_| TargetError::InvalidPath(pid_file.to_path_buf()))?
        .trim()
        .parse::<i32>()
        .map_err(|_| TargetError::InvalidPidFile(pid_file.to_path_buf()))
}

/// Process defined by a pid.
///
/// Once the process is gone, the target returns no metrics.
struct Target<'a> {
    name: String,
    pinfo: Option<ProcessInfo>,
    pid_file: Option<PathBuf>,
    sysconf: &'a SystemConf,
}

impl<'a> Target<'a> {
    fn new(pid: pid_t, sysconf: &'a SystemConf) -> TargetResult<Self> {
        let pinfo = ProcessInfo::with_pid(pid).map_err(|_| TargetError::InvalidProcessId(pid))?;
        Ok(Self {
            name: pinfo.name().to_string(),
            pinfo: Some(pinfo),
            pid_file: None,
            sysconf,
        })
    }

    fn with_pid_file<P>(pid_file: P, sysconf: &'a SystemConf) -> TargetResult<Self>
    where
        P: AsRef<Path>,
    {
        let pid_file = pid_file.as_ref();
        Ok(Self {
            name: basename(pid_file, true)
                .ok_or_else(|| TargetError::InvalidPath(pid_file.to_path_buf()))?,
            pinfo: None,
            pid_file: Some(pid_file.to_path_buf()),
            sysconf,
        })
    }

    fn is_alive(&self) -> bool {
        self.pinfo
            .as_ref()
            .map(|pinfo| pinfo.process().is_alive())
            .unwrap_or(false)
    }

    fn set_process(&mut self, pid: pid_t) -> TargetResult<()> {
        let pinfo = ProcessInfo::with_pid(pid).map_err(|_| TargetError::InvalidProcessId(pid))?;
        self.pinfo = Some(pinfo);
        Ok(())
    }

    fn clear_process(&mut self) -> bool {
        let changed = self.pinfo.is_some();
        self.pinfo = None;
        changed
    }

    fn pid_file(&self) -> Option<&PathBuf> {
        self.pid_file.as_ref()
    }

    fn collect(&self, collector: &mut Collector) {
        if let Some(pinfo) = &self.pinfo {
            collector.collect(&self.name, pinfo, self.sysconf);
        }
    }
}

/// Target container
pub struct TargetContainer<'a> {
    targets: Vec<Target<'a>>,
    sysconf: &'a SystemConf,
    system_limits: Option<Vec<Option<Limit>>>,
    with_system: bool,
}

impl<'a> TargetContainer<'a> {
    pub fn new(sysconf: &'a SystemConf, with_system: bool) -> TargetContainer<'a> {
        TargetContainer {
            targets: Vec::new(),
            sysconf,
            system_limits: None,
            with_system,
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
                    Ok(pid) => match target.set_process(pid) {
                        Ok(()) => changed = true,
                        Err(err) => error!("{pid}: {err:?}"),
                    },
                    Err(err) => error!("{err:?}"),
                }
            }
        });
        changed
    }

    pub fn initialize(&mut self, metric_count: usize) {
        if self.with_system {
            self.system_limits = Some(vec![None; metric_count]);
        }
    }

    pub fn collect(&self, collector: &mut Collector) {
        collector.rewind();
        if let Some(ref limits) = self.system_limits {
            let mut system = SystemStat::new(self.sysconf);
            collector.collect_system(&mut system);
            collector.record(
                "system",
                0,
                None,
                &system.extract_metrics(collector.metrics()),
                limits,
            );
        }
        self.targets
            .iter()
            .for_each(|target| target.collect(collector));
        collector.finish();
    }

    /// Push a target by PID.
    ///
    /// Panic if the target is not a PID or a PID file.
    pub fn push_by_pid(&mut self, target_id: &TargetId) -> TargetResult<()> {
        let target = match target_id {
            TargetId::Pid(pid) => Target::new(*pid, self.sysconf)?,
            TargetId::PidFile(pid_file) => Target::with_pid_file(pid_file, self.sysconf)?,
            _ => panic!("already matched"),
        };
        self.targets.push(target);
        Ok(())
    }

    /// Push a target.
    pub fn push(&mut self, target_id: &TargetId, forest: &ProcessForest) -> TargetResult<()> {
        match target_id {
            TargetId::System => {
                self.with_system = true;
            }
            TargetId::ProcessName(name) => {
                forest.iter_roots().for_each(|p| {
                    if let Ok(descendants) = forest.descendants(p.pid()) {
                        descendants.for_each(|p| {
                            if name == p.name() {
                                match Target::new(p.pid(), self.sysconf) {
                                    Ok(target) => self.targets.push(target),
                                    Err(err) => error!("{name}: {err}"),
                                }
                            }
                        })
                    }
                });
            }
            _ => self.push_by_pid(target_id)?,
        };
        Ok(())
    }

    pub fn push_all(&mut self, target_ids: &[TargetId]) -> TargetResult<()> {
        let forest = {
            let mut forest = ProcessForest::new();
            forest.refresh().map_err(TargetError::ProcessError)?;
            forest
        };

        for target_id in target_ids {
            self.push(target_id, &forest)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use std::path::PathBuf;

    #[test]
    fn test_basename() {
        assert_eq!(
            "file.pid",
            super::basename(PathBuf::from("/a/file.pid"), false).unwrap()
        );
        assert_eq!(
            "file",
            super::basename(PathBuf::from("/a/file.pid"), true).unwrap()
        );
    }
}
