use anyhow::Context;
use libc::pid_t;
use procfs::process::{all_processes, Process, Stat};
use procfs::Meminfo;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::collector::Collector;
use crate::metric::MetricId;

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
        name.map(|name| String::from(name))
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

/// System Filesystem
struct SystemFs {
    _tps: u64,
    meminfo: Option<Meminfo>,
}

impl SystemFs {
    fn new(ticks_per_second: u64) -> SystemFs {
        SystemFs {
            _tps: ticks_per_second,
            meminfo: None,
        }
    }

    fn with_meminfo<F>(&mut self, func: F) -> u64
    where
        F: Fn(&Meminfo) -> u64,
    {
        if self.meminfo.is_none() {
            self.meminfo = Some(Meminfo::new().expect("cannot access /proc/meminfo"));
        }
        match self.meminfo {
            Some(ref meminfo) => func(meminfo),
            None => panic!("internal error"),
        }
    }

    fn extract_metrics(&mut self, ids: &Vec<MetricId>) -> Vec<u64> {
        ids.iter()
            .map(|id| match id {
                MetricId::MemVm => {
                    self.with_meminfo(|mi| mi.mem_total - mi.mem_available.unwrap_or(mi.mem_free))
                }
                _ => 0,
            })
            .collect()
    }
}

/// Extract metrics for a process
struct ProcessFs<'a> {
    process: &'a Process,
    tps: u64,
    stat: Option<Stat>,
}

impl<'a> ProcessFs<'a> {
    fn new(process: &'a Process, ticks_per_second: u64) -> ProcessFs {
        ProcessFs {
            process,
            tps: ticks_per_second,
            stat: None,
        }
    }

    fn stat(&mut self) -> Option<&Stat> {
        if self.stat.is_none() {
            self.stat = self.process.stat().ok();
        }
        self.stat.as_ref()
    }

    fn extract_metrics(&mut self, ids: &Vec<MetricId>) -> Vec<u64> {
        ids.iter()
            .map(|id| match id {
                MetricId::MemVm | MetricId::MemRss | MetricId::TimeSystem | MetricId::TimeUser => {
                    if let Some(stat) = self.stat() {
                        match id {
                            MetricId::MemVm => stat.vsize,
                            MetricId::MemRss => {
                                if stat.rss < 0 {
                                    0
                                } else {
                                    stat.rss as u64
                                }
                            }
                            MetricId::TimeSystem => stat.stime / self.tps,
                            MetricId::TimeUser => stat.utime / self.tps,
                        }
                    } else {
                        0
                    }
                }
            })
            .collect()
    }
}

/// The system itself
struct SystemTarget {
    tps: u64,
}

impl SystemTarget {
    fn new() -> anyhow::Result<SystemTarget> {
        Ok(SystemTarget {
            tps: procfs::ticks_per_second()? as u64,
        })
    }
}

impl Target for SystemTarget {
    fn get_name(&self) -> &str {
        "system"
    }

    fn collect(&self, collector: &mut dyn Collector) {
        let mut system = SystemFs::new(self.tps);
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
struct StaticTarget {
    name: String,
    proc_dir: PathBuf,
    process: Option<Process>,
    tps: u64,
}

impl StaticTarget {
    fn new(pid: pid_t) -> anyhow::Result<StaticTarget> {
        let process = Process::new(pid).map_err(|_| Error::InvalidProcessId(pid))?;
        Ok(StaticTarget {
            name: name_from_process_or_pid(&process),
            proc_dir: proc_dir(pid),
            process: Some(process),
            tps: procfs::ticks_per_second()? as u64,
        })
    }

    fn new_no_error(pid: pid_t) -> StaticTarget {
        StaticTarget::new(pid).unwrap_or_else(|_| StaticTarget {
            name: name_from_pid(pid),
            proc_dir: proc_dir(pid),
            process: None,
            tps: 0,
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
                let mut procfs = ProcessFs::new(process, self.tps);
                collector.collect(
                    name,
                    process.pid(),
                    procfs.extract_metrics(collector.metric_ids()),
                )
            }
            None => collector.no_data(self.get_name()),
        }
    }
}

impl Target for StaticTarget {
    fn get_name(&self) -> &str {
        self.name.as_str()
    }

    fn collect(&self, collector: &mut dyn Collector) {
        self.collect_with_name(self.get_name(), collector);
    }

    fn refresh(&mut self) -> bool {
        if self.has_process() {
            if !self.is_alive() {
                self.process = None;
                return true;
            }
        }
        false
    }
}

/// Process defined by a pid file.
///
/// The pid can change over the time.
struct DynamicTarget {
    name: Option<String>,
    target: Option<StaticTarget>,
    pid_file: PathBuf,
}

impl DynamicTarget {
    fn new(pid_file: &PathBuf) -> DynamicTarget {
        DynamicTarget {
            name: name_from_path(pid_file, true),
            target: read_pid_file(pid_file.as_path())
                .map_or(None, |pid| Some(StaticTarget::new_no_error(pid))),
            pid_file: pid_file.clone(),
        }
    }
}

impl Target for DynamicTarget {
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
                    self.target = Some(StaticTarget::new_no_error(pid));
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
struct MultiTarget {
    name: String,
    targets: Vec<StaticTarget>,
}

impl MultiTarget {
    fn new(name: &str) -> MultiTarget {
        MultiTarget {
            name: name.to_string(),
            targets: Vec::new(),
        }
    }
}

impl Target for MultiTarget {
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
                        if let Ok(target) = StaticTarget::new(process.pid()) {
                            self.targets.push(target);
                        }
                    });
                }
            }
            return count > 0 || self.targets.len() > 0;
        }
        false
    }
}

/// Target holder
enum TargetHolder {
    System(SystemTarget),
    Static(Box<StaticTarget>),
    Dynamic(Box<DynamicTarget>),
    Multi(MultiTarget),
}

/// Target container
pub struct TargetContainer {
    targets: Vec<TargetHolder>,
}

impl TargetContainer {
    pub fn new() -> TargetContainer {
        TargetContainer {
            targets: Vec::new(),
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
            TargetId::Pid(pid) => TargetHolder::Static(Box::new(StaticTarget::new(*pid)?)),
            TargetId::PidFile(pid_file) => {
                TargetHolder::Dynamic(Box::new(DynamicTarget::new(&pid_file)))
            }
            TargetId::ProcessName(name) => TargetHolder::Multi(MultiTarget::new(&name)),
            TargetId::System => TargetHolder::System(SystemTarget::new()?),
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
