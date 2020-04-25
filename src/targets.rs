use anyhow::{Context, Result};
use libc::pid_t;
use procfs::process::Process;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::collectors::Collector;

/// Different way of identifying processes
#[derive(Debug)]
pub enum TargetId {
    Pid(pid_t),
    PidFile(PathBuf),
    ProcessName(String),
}

/// Target process
pub trait Target {
    fn get_name(&self) -> &str;
    fn collect(&self, target_number: usize, collector: &mut dyn Collector);
    fn refresh(&mut self);
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
    if let Some(basename) = basename {
        if let Some(name) = basename.to_str() {
            return Some(String::from(name));
        }
    }
    None
}

fn name_from_process(process: &Process) -> String {
    if let Ok(path) = process.exe() {
        if let Some(name) = name_from_path(&path, false) {
            return name;
        }
    }
    name_from_pid(process.pid)
}

fn read_pid_file(pid_file: &Path) -> Result<pid_t> {
    let mut file = File::open(pid_file)
        .with_context(|| format!("{}: cannot open file", pid_file.display()))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents
        .trim()
        .parse::<i32>()
        .with_context(|| format!("{}: invalid pid file", pid_file.display()))?)
}

/// Non existent process.
struct NullTarget {
    name: String,
}

impl NullTarget {
    fn new(pid: pid_t) -> NullTarget {
        NullTarget {
            name: format!("process[{}]", pid),
        }
    }
}

impl Target for NullTarget {
    fn get_name(&self) -> &str {
        self.name.as_str()
    }

    fn collect(&self, target_number: usize, collector: &mut dyn Collector) {
        collector.collect(target_number, 0, self.get_name(), None);
    }

    fn refresh(&mut self) {}
}

/// Process defined by a pid.
///
/// Once the process is gone, the target returns no metrics.
struct StaticTarget {
    name: String,
    process: Process,
}

impl StaticTarget {
    fn new(process: Process) -> StaticTarget {
        StaticTarget {
            name: name_from_process(&process),
            process: process,
        }
    }
}

impl Target for StaticTarget {
    fn get_name(&self) -> &str {
        self.name.as_str()
    }

    fn collect(&self, target_number: usize, collector: &mut dyn Collector) {
        collector.collect(target_number, 0, self.get_name(), Some(&self.process));
    }

    fn refresh(&mut self) {}
}

/// Process defined by a pid file.
///
/// The pid can change over the time.
struct DynamicTarget {
    name: String,
    pid_file: PathBuf,
    process: Option<Process>,
}

impl DynamicTarget {
    fn new(pid_file: &PathBuf) -> DynamicTarget {
        DynamicTarget {
            name: name_from_path(pid_file, true).unwrap_or(String::from("<unknown>")),
            pid_file: pid_file.clone(),
            process: None,
        }
    }
}

impl Target for DynamicTarget {
    fn get_name(&self) -> &str {
        self.name.as_str()
    }

    fn collect(&self, target_number: usize, collector: &mut dyn Collector) {
        collector.collect(target_number, 0, self.get_name(), self.process.as_ref());
    }

    fn refresh(&mut self) {
        if self.process.is_none() {
            if let Ok(pid) = read_pid_file(self.pid_file.as_path()) {
                self.process = Process::new(pid).ok();
            }
        }
    }
}

/// Process defined by name.
///
/// There may be multiple instances. The target sums the metrics.
struct MultiTarget {
    name: String,
    processes: Vec<Process>,
}

impl MultiTarget {
    fn new(name: &String) -> MultiTarget {
        MultiTarget {
            name: name.clone(),
            processes: Vec::new(),
        }
    }
}

impl Target for MultiTarget {
    fn get_name(&self) -> &str {
        self.name.as_str()
    }

    fn collect(&self, target_number: usize, collector: &mut dyn Collector) {
        for (process_number, process) in self.processes.iter().enumerate() {
            collector.collect(
                target_number,
                process_number,
                self.get_name(),
                Some(&process),
            );
        }
    }

    fn refresh(&mut self) {
        panic!("not implemented");
    }
}

/// Target holder
enum TargetHolder {
    Null(NullTarget),
    Static(StaticTarget),
    Dynamic(DynamicTarget),
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

    pub fn collect(&self, collector: &mut dyn Collector) {
        collector.clear();
        self.targets
            .iter()
            .enumerate()
            .for_each(|(target_number, holder)| match holder {
                TargetHolder::Null(target) => target.collect(target_number, collector),
                TargetHolder::Static(target) => target.collect(target_number, collector),
                TargetHolder::Dynamic(target) => target.collect(target_number, collector),
                TargetHolder::Multi(target) => target.collect(target_number, collector),
            })
    }

    pub fn push(&mut self, target_id: &TargetId) {
        self.targets.push(match target_id {
            TargetId::Pid(pid) => match Process::new(*pid) {
                Ok(process) => TargetHolder::Static(StaticTarget::new(process)),
                Err(_) => TargetHolder::Null(NullTarget::new(*pid)),
            },
            TargetId::PidFile(pid_file) => TargetHolder::Dynamic(DynamicTarget::new(&pid_file)),
            TargetId::ProcessName(name) => TargetHolder::Multi(MultiTarget::new(&name)),
        });
    }

    pub fn push_all(&mut self, target_ids: &Vec<TargetId>) {
        for target_id in target_ids {
            self.push(target_id);
        }
    }
}
