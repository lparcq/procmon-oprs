// Oprs -- process monitor for Linux
// Copyright (C) 2020-2025  Laurent Pelecq
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

use std::io::{self, BufRead, BufReader, Write};
use std::iter::IntoIterator;
use std::path::Path;
use std::time::Duration;

#[cfg(not(test))]
mod process {
    use std::io::Result;
    use std::path::Path;

    pub use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

    pub fn spawn<P>(command: &str, working_dir: P) -> Result<Child>
    where
        P: AsRef<Path>,
    {
        Command::new(command)
            .arg("-")
            .current_dir(working_dir.as_ref())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
    }
}

#[cfg(test)]
mod process {
    use std::io::{Cursor, Result, Seek, Write};
    use std::path::Path;

    pub struct ChildStdin {}
    impl Write for ChildStdin {
        fn write(&mut self, _buf: &[u8]) -> Result<usize> {
            Ok(0)
        }
        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
    }

    pub type ChildStdout = Cursor<Vec<u8>>;

    pub struct Child {
        pub stdin: Option<ChildStdin>,
        pub stdout: Option<ChildStdout>,
    }

    impl Child {
        pub fn wait(&self) -> Result<()> {
            Ok(())
        }
    }

    pub fn spawn<P>(output: &str, _path: P) -> Result<Child>
    where
        P: AsRef<Path>,
    {
        let mut buf = Cursor::new(Vec::<u8>::new());
        writeln!(buf, "{output}")?;
        buf.rewind()?;
        Ok(Child {
            stdin: Some(ChildStdin {}),
            stdout: Some(buf),
        })
    }
}

use process::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("rrdtool: no standard input for subprocess")]
    NoStdin,
    #[error("rrdtool: no standard output for subprocess")]
    NoStdout,
    #[error("rrdtool: premature end of stream")]
    EndOfStream,
    #[error("rrdtool: invalid graph size: {0}")]
    InvalidGraphSize(String),
    #[error("rrdtool: missing graph size")]
    MissingGraphSize,
    #[error("rrdtool: {0}")]
    Process(String),
    #[error("rrdtool: input/output error: {0}")]
    Io(io::Error),
}

macro_rules! try_io {
    // Assign option to lvalue if option is set.
    ($res:expr) => {
        $res.map_err(Error::Io)?
    };
}

macro_rules! try_write {
    ($file:expr, $fmt:expr) => {
        try_io!(write!($file, $fmt))
    };
    ($file:expr, $fmt:expr, $($arg:tt)*) => {
        try_io!(write!($file, $fmt, $($arg)*))
    };
}

macro_rules! try_writeln {
    ($file:expr) => {
        try_io!(writeln!($file))
    };
    ($file:expr, $fmt:expr) => {
        try_io!(writeln!($file, $fmt))
    };
    ($file:expr, $fmt:expr, $($arg:tt)*) => {
        try_io!(writeln!($file, $fmt, $($arg)*))
    };
}

fn parse_graph_size(line: &str) -> Result<(usize, usize), Error> {
    let size = line.trim_end();
    let mut tokens = size.splitn(2, 'x');
    if let Some(first) = tokens.next()
        && let Ok(width) = first.parse::<usize>()
        && let Some(second) = tokens.next()
        && let Ok(height) = second.parse::<usize>()
    {
        return Ok((width, height));
    }
    Err(Error::InvalidGraphSize(size.to_string()))
}

pub struct RrdTool {
    process: Child,
    child_in: ChildStdin,
    child_out: BufReader<ChildStdout>,
}

impl RrdTool {
    pub fn new<P>(working_dir: P) -> Result<RrdTool, Error>
    where
        P: AsRef<Path>,
    {
        let mut process = try_io!(spawn("rrdtool", working_dir.as_ref()));
        let child_out = process.stdout.take().ok_or(Error::NoStdout)?;
        let child_in = process.stdin.take().ok_or(Error::NoStdin)?;
        Ok(RrdTool {
            process,
            child_in,
            child_out: BufReader::new(child_out),
        })
    }

    /// Read a lines until the status line from a rrdtool subprocess
    fn read_answer(&mut self, mut lines: Option<&mut Vec<String>>) -> Result<(), Error> {
        loop {
            let mut line = String::new();
            try_io!(self.child_out.read_line(&mut line));
            let answer = line.trim_end();
            let mut tokens = answer.splitn(2, ' ');
            let tag = tokens.next().ok_or(Error::EndOfStream)?;
            match tag {
                "OK" => return Ok(()),
                "ERROR:" => {
                    return Err(Error::Process(String::from(
                        tokens.next().unwrap_or("no error message"),
                    )));
                }
                _ => {
                    if let Some(ref mut lines) = lines {
                        lines.push(answer.to_string());
                    }
                }
            }
        }
    }

    /// Create a Round-Robin database
    pub fn create<I, S>(
        &mut self,
        dbname: &str,
        ds: I,
        start_time: &Duration,
        interval: &Duration,
        rows: usize,
    ) -> Result<(), Error>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let step = interval.as_secs();
        log::debug!("rrd create {dbname} step={step}");
        try_io!(write!(
            self.child_in,
            "create {} --start={} --step={}",
            dbname,
            start_time.as_secs(),
            step
        ));
        for ds in ds.into_iter() {
            try_io!(write!(self.child_in, " {}", ds.as_ref()));
        }
        try_io!(writeln!(self.child_in, " RRA:AVERAGE:0.5:1:{rows}"));
        self.read_answer(None)
    }

    /// Update values
    pub fn update<I>(&mut self, dbname: &str, values: I, timestamp: &Duration) -> Result<(), Error>
    where
        I: std::iter::Iterator<Item = u64>,
    {
        log::debug!("rrd update {dbname}");
        try_write!(self.child_in, "update {} {}", dbname, timestamp.as_secs());
        for value in values {
            try_write!(self.child_in, ":{}", value);
        }
        try_writeln!(self.child_in);
        self.read_answer(None)
    }

    /// Generate a graph file and return it's size.
    pub fn graph<I, S>(
        &mut self,
        filename: &str,
        start_time: &Duration,
        end_time: &Duration,
        defs: I,
        title: Option<&str>,
    ) -> Result<(usize, usize), Error>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let start = start_time.as_secs();
        let end = end_time.as_secs();
        log::debug!("rrd graph {filename} --start={start} --end={end}");
        try_write!(
            self.child_in,
            "graph {filename} --start={start} --end={end}",
        );
        if let Some(title) = title {
            try_write!(self.child_in, " --title=\"{title}\"");
        }
        for def in defs.into_iter() {
            try_write!(self.child_in, " {}", def.as_ref());
        }
        try_writeln!(self.child_in);
        let mut lines = Vec::new();
        self.read_answer(Some(&mut lines))?;
        if lines.is_empty() {
            Err(Error::MissingGraphSize)
        } else {
            parse_graph_size(&lines[0])
        }
    }

    pub fn close(&mut self) -> Result<(), Error> {
        log::debug!("stopping rrdtool");
        try_writeln!(self.child_in, "quit");
        log::debug!("waiting for rrdtool to stop");
        try_io!(self.process.wait());
        log::debug!("rrdtool stopped");
        Ok(())
    }
}

#[cfg(test)]
mod test {

    use std::io::{self, BufReader};
    use std::path::PathBuf;

    use super::{Error, RrdTool, parse_graph_size, spawn};

    fn new_tool(output: &str) -> io::Result<RrdTool> {
        let mut process = spawn(output, PathBuf::new())?;
        let child_in = process.stdin.take().unwrap();
        let child_out = process.stdout.take().unwrap();
        Ok(RrdTool {
            process,
            child_in,
            child_out: BufReader::new(child_out),
        })
    }

    #[test]
    fn parse_rrdtool_answer() -> Result<(), Error> {
        let mut tool_ok_no_capture = try_io!(new_tool("OK u:0,01 s:0,02 r:8,05"));
        tool_ok_no_capture.read_answer(None)?;

        let mut tool_ok = try_io!(new_tool("OK u:0,01 s:0,02 r:8,05"));
        let mut lines_ok = Vec::new();
        tool_ok.read_answer(Some(&mut lines_ok))?;
        assert!(lines_ok.is_empty());

        let mut tool_err = try_io!(new_tool(
            "ERROR: you must define at least one Round Robin Archive"
        ));
        let mut lines_err = Vec::new();
        match tool_err.read_answer(Some(&mut lines_err)) {
            Ok(()) => panic!("rrdtool error not correctly parsed"),
            Err(err) => {
                assert_eq!(
                    "rrdtool: you must define at least one Round Robin Archive",
                    format!("{err}")
                );
                assert!(lines_err.is_empty());
            }
        }

        let mut tool_graph = try_io!(new_tool("481x155\nOK u:0,07 s:0,01 r:0,06"));
        let mut lines_graph = Vec::new();
        tool_graph.read_answer(Some(&mut lines_graph))?;
        assert_eq!(1, lines_graph.len());
        assert_eq!("481x155", lines_graph[0]);
        Ok(())
    }

    #[test]
    fn parse_graph_sizes() -> Result<(), Error> {
        let (width, height) = parse_graph_size("481x155\n")?;
        assert_eq!(481, width);
        assert_eq!(155, height);

        assert!(parse_graph_size("1x2x3\n").is_err());
        Ok(())
    }
}
