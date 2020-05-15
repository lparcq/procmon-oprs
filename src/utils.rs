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

use anyhow::Context;
use libc::pid_t;
use procfs::process::Process;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

#[cfg(not(test))]
use std::fs;

#[cfg(test)]
use crate::mocks::fs;

/// Name identifying a process if only the pid is known
pub fn name_from_pid(pid: pid_t) -> String {
    format!("[{}]", pid)
}

/// Name identifying a process from its program path
pub fn name_from_path(path: &PathBuf, no_extension: bool) -> Option<String> {
    let basename: Option<&std::ffi::OsStr> = if no_extension {
        path.file_stem()
    } else {
        path.file_name()
    };
    basename.and_then(|name| name.to_str()).map(String::from)
}

/// Name identifying a process
pub fn name_from_process(process: &Process) -> Option<String> {
    if let Ok(path) = process.exe() {
        if let Some(name) = name_from_path(&path, false) {
            return Some(name);
        }
    }
    None
}

/// Name identifying a process (default based on pid)
pub fn name_from_process_or_pid(process: &Process) -> String {
    name_from_process(process).unwrap_or_else(|| name_from_pid(process.pid()))
}

/// Read a PID file and returns the PID it contains
pub fn read_pid_file(pid_file: &Path) -> anyhow::Result<pid_t> {
    let mut file = fs::File::open(pid_file)
        .with_context(|| format!("{}: cannot open file", pid_file.display()))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents
        .trim()
        .parse::<i32>()
        .with_context(|| format!("{}: invalid pid file", pid_file.display()))?)
}

/// Process directory
pub fn proc_dir(pid: pid_t) -> PathBuf {
    PathBuf::from("/proc").join(format!("{}", pid))
}

/// Read the first string in a file
pub fn read_file_first_string<P>(path: P, end_char: u8) -> Option<String>
where
    P: AsRef<Path>,
{
    if let Ok(file) = fs::File::open(path) {
        let mut string_buf = Vec::new();
        if let Ok(size) = BufReader::new(file).read_until(end_char, &mut string_buf) {
            if size > 0 {
                string_buf.truncate(size - 1); // remove end char
                return String::from_utf8(string_buf).ok();
            }
        }
    }
    None
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

    #[test]
    fn test_read_file_first_string() {
        let path = PathBuf::from("content:/a/b\tone\ttwo");
        match super::read_file_first_string(path, b'\t') {
            Some(value) => assert_eq!("/a/b", value),
            None => panic!("no string"),
        }
    }
}
