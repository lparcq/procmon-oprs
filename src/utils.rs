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
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

#[cfg(not(test))]
use std::fs;

#[cfg(test)]
use crate::mocks::fs;

/// Base name of a file with or without extension
pub fn basename<P>(path: P, no_extension: bool) -> Option<String>
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
    fn test_basename() {
        assert_eq!(
            "file.pid",
            super::basename(&PathBuf::from("/a/file.pid"), false).unwrap()
        );
        assert_eq!(
            "file",
            super::basename(&PathBuf::from("/a/file.pid"), true).unwrap()
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
