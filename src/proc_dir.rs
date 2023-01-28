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

// Various utility functions

use libc::pid_t;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::utils::{basename, read_file_first_string};

/// Wrapper on a process directory
pub struct ProcessDir<'a> {
    dir: &'a Path,
}

impl<'a> ProcessDir<'a> {
    pub fn new(dir: &'a Path) -> ProcessDir<'a> {
        ProcessDir { dir }
    }

    pub fn path(pid: pid_t) -> PathBuf {
        PathBuf::from("/proc").join(format!("{pid}"))
    }

    /// Name from the process command line first argument
    pub fn process_name_from_cmdline(&self) -> Option<String> {
        if let Some(first_string) = read_file_first_string(self.dir.join("cmdline"), b'\0') {
            PathBuf::from(first_string)
                .file_name()
                .and_then(|os_name| os_name.to_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    }

    /// Name from exe path
    pub fn process_name_from_exe(&self) -> Option<String> {
        match fs::read_link(self.dir.join("exe")) {
            Ok(path) => basename(path, false),
            Err(_) => None,
        }
    }

    /// Name from exe or process command line
    pub fn process_name(&self) -> Option<String> {
        self.process_name_from_exe()
            .or_else(|| self.process_name_from_cmdline())
    }

    /// Name identifying a process if only the pid is known
    pub fn process_name_from_pid(pid: pid_t) -> String {
        format!("[{pid}]")
    }
}

/// Find pids of processes by name
pub struct PidFinder<'a, 'b> {
    pids: &'a mut Vec<Vec<pid_t>>,
    index: HashMap<&'b str, usize>,
}

impl<'a, 'b> PidFinder<'a, 'b> {
    pub fn new(pids: &'a mut Vec<Vec<pid_t>>) -> PidFinder<'a, 'b> {
        PidFinder {
            pids,
            index: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: &'b str) {
        self.index.insert(name, self.pids.len());
        self.pids.push(Vec::new());
    }

    pub fn insert(&mut self, name: &str, pid: pid_t) -> bool {
        if let Some(index) = self.index.get(name) {
            self.pids[*index].push(pid);
            true
        } else {
            false
        }
    }

    pub fn fill(&mut self) {
        if let Ok(entries) = fs::read_dir("/proc") {
            for entry in entries.into_iter().flatten() {
                if let Ok(pid) = i32::from_str(&entry.file_name().to_string_lossy()) {
                    let path = entry.path();
                    let proc_dir = ProcessDir::new(path.as_path());
                    if let Some(name) = proc_dir.process_name_from_exe() {
                        if self.insert(name.as_str(), pid) {
                            continue;
                        }
                    }
                    if let Some(name) = proc_dir.process_name_from_cmdline() {
                        let _ = self.insert(name.as_str(), pid);
                    }
                }
            }
        }
    }
}
