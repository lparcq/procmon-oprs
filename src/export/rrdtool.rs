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

use log::debug;
use std::io::{BufRead, BufReader, Write};
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
    use std::io::{self, Cursor, Result, Seek, Write};
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
        write!(buf, "{}\n", output)?;
        buf.seek(io::SeekFrom::Start(0))?;
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
    RrdToolNoStdin,
    #[error("rrdtool: no standard output for subprocess")]
    RrdToolNoStdout,
    #[error("rrdtool: premature end of stream")]
    RrdToolEndOfStream,
    #[error("rrdtool: {0}")]
    RrdToolError(String),
    #[error("rrdtool: unexpected answer: {0}")]
    RrdToolUnexpectedAnswer(String),
}

pub struct RrdTool {
    process: Child,
    child_in: ChildStdin,
    child_out: BufReader<ChildStdout>,
}

impl RrdTool {
    pub fn new<P>(working_dir: P) -> anyhow::Result<RrdTool>
    where
        P: AsRef<Path>,
    {
        let mut process = spawn("rrdtool", working_dir.as_ref())?;
        let child_out = process.stdout.take().ok_or(Error::RrdToolNoStdout)?;
        let child_in = process.stdin.take().ok_or(Error::RrdToolNoStdin)?;
        Ok(RrdTool {
            process,
            child_in,
            child_out: BufReader::new(child_out),
        })
    }

    /// Read a single line answer from a rrdtool subprocess
    fn read_answer(&mut self) -> anyhow::Result<()> {
        let mut answer = String::new();
        self.child_out.read_line(&mut answer)?;
        let mut tokens = answer.trim_end().splitn(2, ' ');
        let tag = tokens.next().ok_or(Error::RrdToolEndOfStream)?;
        match tag {
            "OK" => Ok(()),
            "ERROR:" => Err(Error::RrdToolError(String::from(
                tokens.next().unwrap_or("no error message"),
            )))?,
            _ => Err(Error::RrdToolUnexpectedAnswer(tag.to_string()))?,
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
    ) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let step = interval.as_secs();
        debug!("rrd create {} step={}", dbname, step);
        write!(
            self.child_in,
            "create {} --start={} --step={}",
            dbname,
            start_time.as_secs(),
            step
        )?;
        for ds in ds.into_iter() {
            write!(self.child_in, " {}", ds.as_ref())?;
        }
        writeln!(self.child_in, " RRA:AVERAGE:0.5:1:{}", rows)?;
        self.read_answer()
    }

    /// Send values to a rrdtool subprocess
    pub fn update<'s, I>(
        &mut self,
        dbname: &str,
        values: I,
        timestamp: &Duration,
    ) -> anyhow::Result<()>
    where
        I: std::iter::Iterator<Item = u64>,
    {
        debug!("rrd update {}", dbname);
        write!(self.child_in, "update {} {}", dbname, timestamp.as_secs())?;
        for value in values.into_iter() {
            write!(self.child_in, ":{}", value)?;
        }
        write!(self.child_in, "\n")?;
        self.read_answer()
    }

    pub fn close(&mut self) -> anyhow::Result<()> {
        debug!("stopping rrdtool");
        self.child_in.write_all(b"quit\n")?;
        debug!("waiting for rrdtool to stop");
        self.process.wait()?;
        debug!("rrdtool stopped");
        Ok(())
    }
}

#[cfg(test)]
mod test {

    use std::io::{self, BufReader};
    use std::path::PathBuf;

    use super::{spawn, RrdTool};

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
    fn parse_rrdtool_answer() -> anyhow::Result<()> {
        let mut tool_ok = new_tool("OK u:0,01 s:0,02 r:8,05")?;
        tool_ok.read_answer()?;

        let mut tool_err = new_tool("ERROR: you must define at least one Round Robin Archive")?;
        match tool_err.read_answer() {
            Ok(()) => panic!("rrdtool error not correctly parsed"),
            Err(err) => {
                let msg = format!("{:?}", err);
                assert_eq!(
                    "rrdtool: you must define at least one Round Robin Archive",
                    msg.as_str()
                );
            }
        }
        Ok(())
    }
}
