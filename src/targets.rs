use libc::pid_t;
use procfs::process::Process;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::str::FromStr;
use thiserror::Error;

use crate::collector::Collector;
use crate::info::{ProcessInfo, SystemConf, SystemInfo};
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
    fn collect(&self, collector: &mut dyn Collector);
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

    fn collect(&self, collector: &mut dyn Collector) {
        let mut system = SystemInfo::new(self.system_conf);
        collector.collect(
            self.get_name(),
            0,
            system.extract_metrics(collector.metric_ids()),
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
    fn new(pid: pid_t, system_conf: &'a SystemConf) -> anyhow::Result<StaticTarget<'a>> {
        let process = Process::new(pid).map_err(|_| Error::InvalidProcessId(pid))?;
        Ok(StaticTarget {
            name: name_from_process_or_pid(&process),
            proc_dir: proc_dir(pid),
            process: Some(process),
            system_conf,
        })
    }

    fn new_no_error(pid: pid_t, system_conf: &'a SystemConf) -> StaticTarget<'a> {
        StaticTarget::new(pid, system_conf).unwrap_or_else(|_| StaticTarget {
            name: name_from_pid(pid),
            proc_dir: proc_dir(pid),
            process: None,
            system_conf,
        })
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

    fn collect_with_name(&self, name: &str, collector: &mut dyn Collector) {
        if let Some(ref process) = self.process {
            let mut proc_info = ProcessInfo::new(process, self.system_conf);
            collector.collect(
                name,
                process.pid(),
                proc_info.extract_metrics(collector.metric_ids()),
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

    fn collect(&self, collector: &mut dyn Collector) {
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
            name: name_from_path(pid_file, true),
            target: read_pid_file(pid_file.as_path()).map_or(None, |pid| {
                Some(StaticTarget::new_no_error(pid, system_conf))
            }),
            pid_file: pid_file.clone(),
            system_conf,
        }
    }

    fn refresh(&mut self) -> bool {
        match &mut self.target {
            Some(target) => target.refresh(),
            None => {
                if let Ok(pid) = read_pid_file(self.pid_file.as_path()) {
                    self.target = Some(StaticTarget::new_no_error(pid, self.system_conf));
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

    fn collect(&self, collector: &mut dyn Collector) {
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
    index: usize,
    targets: Vec<StaticTarget<'a>>,
    system_conf: &'a SystemConf,
}

impl<'a> MultiTarget<'a> {
    fn new(name: &str, index: usize, system_conf: &'a SystemConf) -> MultiTarget<'a> {
        MultiTarget {
            name: name.to_string(),
            index,
            targets: Vec::new(),
            system_conf,
        }
    }

    fn get_index(&self) -> usize {
        self.index
    }

    fn refresh(&mut self, current_pids: &[pid_t]) -> bool {
        // Only parse all processes if there are none or if one has died
        let count = self.targets.len();
        let mut previous_pids = BTreeSet::new();
        self.targets.retain(|target| {
            if let Some(pid) = target.get_pid() {
                previous_pids.insert(pid);
                target.is_alive()
            } else {
                false
            }
        });
        for pid in current_pids {
            if !previous_pids.contains(pid) {
                if let Ok(target) = StaticTarget::new(*pid, self.system_conf) {
                    self.targets.push(target);
                }
            }
        }
        self.targets.len() != count
    }
}

impl<'a> Target for MultiTarget<'a> {
    fn get_name(&self) -> &str {
        self.name.as_str()
    }

    fn collect(&self, collector: &mut dyn Collector) {
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

    /// For multiple targets, find the corresponding pids
    fn get_target_pids(&self, target_pids: &mut Vec<Vec<pid_t>>) {
        let mut target_indexes = HashMap::<&str, usize>::new();
        self.multis.iter().for_each(|target| {
            target_indexes.insert(target.get_name(), target.get_index());
            target_pids.push(Vec::new());
        });
        if let Ok(entries) = std::fs::read_dir("/proc") {
            for entry in entries {
                if let Ok(entry) = entry {
                    if let Ok(pid) = i32::from_str(&entry.file_name().to_string_lossy()) {
                        if let Some(first_string) =
                            read_file_first_string(entry.path().join("cmdline"), b'\0')
                        {
                            if let Some(name) = PathBuf::from(first_string)
                                .file_name()
                                .and_then(|os_name| os_name.to_str())
                            {
                                if let Some(index) = target_indexes.get(name) {
                                    target_pids[*index].push(pid);
                                }
                            }
                        }
                    }
                }
            }
        }
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
            let mut target_pids = Vec::new();
            self.get_target_pids(&mut target_pids);
            self.multis.iter_mut().for_each(|target| {
                if target.refresh(&target_pids[target.get_index()]) {
                    changed = true;
                }
            });
        }
        changed
    }

    pub fn collect(&self, collector: &mut dyn Collector) {
        collector.clear();
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
    }

    pub fn push(&mut self, target_id: &TargetId) -> anyhow::Result<()> {
        match target_id {
            TargetId::System => self.system = Some(SystemTarget::new(self.system_conf)?),
            TargetId::Pid(pid) => self
                .statics
                .push(StaticTarget::new(*pid, self.system_conf)?),
            TargetId::PidFile(pid_file) => self
                .dynamics
                .push(DynamicTarget::new(&pid_file, self.system_conf)),
            TargetId::ProcessName(name) => self.multis.push(MultiTarget::new(
                name.as_str(),
                self.multis.len(),
                self.system_conf,
            )),
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
