use anyhow::Context;
use libc::pid_t;
use procfs::process::{all_processes, Process};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::collector::Collector;
use crate::info::{ProcessInfo, SystemConf, SystemInfo};

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
    fn refresh(&mut self) -> bool;
}

// Utilities

fn name_from_pid(pid: pid_t) -> String {
    format!("[{}]", pid)
}

fn name_from_path(path: &PathBuf, no_extension: bool) -> Option<String> {
    let basename: Option<&std::ffi::OsStr> = if no_extension {
        path.file_stem()
    } else {
        path.file_name()
    };
    if let Some(name) = basename.map(|name| name.to_str()) {
        name.map(String::from)
    } else {
        None
    }
}

fn name_from_process(process: &Process) -> Option<String> {
    if let Ok(path) = process.exe() {
        if let Some(name) = name_from_path(&path, false) {
            return Some(name);
        }
    }
    None
}

fn name_from_process_or_pid(process: &Process) -> String {
    name_from_process(process).unwrap_or_else(|| name_from_pid(process.pid()))
}

fn read_pid_file(pid_file: &Path) -> anyhow::Result<pid_t> {
    let mut file = File::open(pid_file)
        .with_context(|| format!("{}: cannot open file", pid_file.display()))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents
        .trim()
        .parse::<i32>()
        .with_context(|| format!("{}: invalid pid file", pid_file.display()))?)
}

fn proc_dir(pid: pid_t) -> PathBuf {
    PathBuf::from("/proc").join(format!("{}", pid))
}

/// Check if a process has a given name
struct ProcessNameMatcher<'a> {
    name: &'a str,
}

impl<'a> ProcessNameMatcher<'a> {
    fn has_name(&self, process: &Process) -> bool {
        match name_from_process(process) {
            Some(name) => name == self.name,
            None => false,
        }
    }
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

    fn refresh(&mut self) -> bool {
        false
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

    fn collect_with_name(&self, name: &str, collector: &mut dyn Collector) {
        match self.process {
            Some(ref process) => {
                let mut proc_info = ProcessInfo::new(process, self.system_conf);
                collector.collect(
                    name,
                    process.pid(),
                    proc_info.extract_metrics(collector.metric_ids()),
                )
            }
            None => collector.no_data(self.get_name()),
        }
    }
}

impl<'a> Target for StaticTarget<'a> {
    fn get_name(&self) -> &str {
        self.name.as_str()
    }

    fn collect(&self, collector: &mut dyn Collector) {
        self.collect_with_name(self.get_name(), collector);
    }

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
        } else {
            collector.no_data(self.get_name());
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

    fn refresh(&mut self) -> bool {
        // Only parse all processes if there are none or if one has died
        let count = self.targets.len();
        self.targets.retain(|target| target.is_alive());
        if count == 0 || self.targets.len() < count {
            if let Ok(mut processes) = all_processes() {
                let name_matcher = ProcessNameMatcher {
                    name: self.name.as_str(),
                };
                processes.retain(|process| name_matcher.has_name(&process));
                if !processes.is_empty() {
                    processes.iter().for_each(|process| {
                        if let Ok(target) = StaticTarget::new(process.pid(), self.system_conf) {
                            self.targets.push(target);
                        }
                    });
                }
            }
            return count > 0 || !self.targets.is_empty();
        }
        false
    }
}

/// Target holder
enum TargetHolder<'a> {
    System(SystemTarget<'a>),
    Static(Box<StaticTarget<'a>>),
    Dynamic(Box<DynamicTarget<'a>>),
    Multi(MultiTarget<'a>),
}

/// Target container
pub struct TargetContainer<'a> {
    targets: Vec<TargetHolder<'a>>,
    system_conf: &'a SystemConf,
}

impl<'a> TargetContainer<'a> {
    pub fn new(system_conf: &'a SystemConf) -> TargetContainer<'a> {
        TargetContainer {
            targets: Vec::new(),
            system_conf,
        }
    }

    pub fn refresh(&mut self) -> bool {
        let mut changed = false;
        self.targets.iter_mut().for_each(|holder| match holder {
            TargetHolder::Dynamic(ref mut target) => {
                if target.refresh() {
                    changed = true;
                }
            }
            TargetHolder::Multi(ref mut target) => {
                if target.refresh() {
                    changed = true;
                }
            }
            _ => (),
        });
        changed
    }

    pub fn collect(&self, collector: &mut dyn Collector) {
        collector.clear();
        self.targets.iter().for_each(|holder| match holder {
            TargetHolder::Static(target) => target.collect(collector),
            TargetHolder::Dynamic(target) => target.collect(collector),
            TargetHolder::Multi(target) => target.collect(collector),
            TargetHolder::System(target) => target.collect(collector),
        })
    }

    pub fn push(&mut self, target_id: &TargetId) -> anyhow::Result<()> {
        self.targets.push(match target_id {
            TargetId::Pid(pid) => {
                TargetHolder::Static(Box::new(StaticTarget::new(*pid, self.system_conf)?))
            }
            TargetId::PidFile(pid_file) => {
                TargetHolder::Dynamic(Box::new(DynamicTarget::new(&pid_file, self.system_conf)))
            }
            TargetId::ProcessName(name) => {
                TargetHolder::Multi(MultiTarget::new(&name, self.system_conf))
            }
            TargetId::System => TargetHolder::System(SystemTarget::new(self.system_conf)?),
        });
        Ok(())
    }

    pub fn push_all(&mut self, target_ids: &[TargetId]) -> anyhow::Result<()> {
        for target_id in target_ids {
            self.push(target_id)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use std::path::PathBuf;

    #[test]
    fn test_name_from_path() {
        assert_eq!(
            "file.pid",
            super::name_from_path(&PathBuf::from("/a/file.pid"), false).unwrap()
        );
        assert_eq!(
            "file",
            super::name_from_path(&PathBuf::from("/a/file.pid"), true).unwrap()
        );
    }
}
