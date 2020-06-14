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

use libc::pid_t;
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("rrd: interval too large")]
    IntervalTooLarge,
    #[error("rrdtool: premature end of stream")]
    RrdToolEndOfStream,
    #[error("rrdtool: {0}")]
    RrdToolError(String),
    #[error("rrdtool: unexpected answer: {0}")]
    RrdToolUnexpectedAnswer(String),
}

use crate::{agg::Aggregation, collector::Collector, metrics::MetricId};

use super::Exporter;

enum DataSourceType {
    Counter,
    Gauge,
}

fn data_source_type(id: MetricId) -> DataSourceType {
    match id {
        MetricId::FaultMinor | MetricId::FaultMajor => DataSourceType::Counter,
        MetricId::FdAll
        | MetricId::FdHigh
        | MetricId::FdFile
        | MetricId::FdSocket
        | MetricId::FdNet
        | MetricId::FdPipe
        | MetricId::FdAnon
        | MetricId::FdMemFile
        | MetricId::FdOther => DataSourceType::Gauge,
        MetricId::IoReadCall
        | MetricId::IoReadCount
        | MetricId::IoReadStorage
        | MetricId::IoWriteCall
        | MetricId::IoWriteCount
        | MetricId::IoWriteStorage => DataSourceType::Counter,
        MetricId::MapAnonSize
        | MetricId::MapAnonCount
        | MetricId::MapHeapSize
        | MetricId::MapHeapCount
        | MetricId::MapFileSize
        | MetricId::MapFileCount
        | MetricId::MapStackSize
        | MetricId::MapStackCount
        | MetricId::MapThreadStackSize
        | MetricId::MapThreadStackCount
        | MetricId::MapVdsoSize
        | MetricId::MapVdsoCount
        | MetricId::MapVsyscallSize
        | MetricId::MapVsyscallCount
        | MetricId::MapVvarSize
        | MetricId::MapVvarCount
        | MetricId::MapOtherSize
        | MetricId::MapOtherCount => DataSourceType::Gauge,
        MetricId::MemRss | MetricId::MemVm | MetricId::MemText | MetricId::MemData => {
            DataSourceType::Gauge
        }
        MetricId::TimeElapsed | MetricId::TimeCpu | MetricId::TimeSystem | MetricId::TimeUser => {
            DataSourceType::Counter
        }
        MetricId::ThreadCount => DataSourceType::Gauge,
    }
}

fn read_rrdtool_answer<R: BufRead>(stdout: &mut R) -> anyhow::Result<()> {
    let mut answer = String::new();
    stdout.read_line(&mut answer)?;
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

pub struct RrdExporter {
    interval: Duration,
    size: usize,
    tool: Child,
    child_in: ChildStdin,
    child_out: BufReader<ChildStdout>,
    ds: Vec<String>,
    skip: Vec<bool>,
    pids: HashMap<pid_t, String>,
}

impl RrdExporter {
    pub fn new<P>(dir: P, interval: Duration, size: usize) -> io::Result<RrdExporter>
    where
        P: AsRef<Path>,
    {
        let mut tool = Command::new("rrdtool")
            .arg("-")
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let child_out = tool
            .stdout
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "rrdtool: no standard input"))?;
        let child_in = tool
            .stdin
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "rrdtool: no standard output"))?;
        Ok(RrdExporter {
            interval,
            size,
            tool,
            child_in,
            child_out: BufReader::new(child_out),
            ds: Vec::new(),
            skip: Vec::new(),
            pids: HashMap::new(),
        })
    }

    fn filename(pid: pid_t, name: &str) -> String {
        format!("{}_{}.rrd", name, pid)
    }

    fn read_answer(&mut self) -> anyhow::Result<()> {
        read_rrdtool_answer(&mut self.child_out)
    }
}

impl Exporter for RrdExporter {
    fn open(&mut self, collector: &Collector) -> anyhow::Result<()> {
        let heart_beat = self.interval.as_secs() * 2;
        collector.for_each_computed_metric(|id, agg| {
            let ds_name = id.to_str().replace(":", "_");
            let ds_type = match data_source_type(id) {
                DataSourceType::Counter => "COUNTER",
                DataSourceType::Gauge => "GAUGE",
            };
            if let Aggregation::None = agg {
                self.skip.push(false);
                self.ds.push(format!(
                    "DS:{}:{}:{}:0.5:1:{}",
                    ds_name, ds_type, heart_beat, self.size,
                ));
            } else {
                self.skip.push(true);
            }
        });
        Ok(())
    }

    fn close(&mut self) -> anyhow::Result<()> {
        if let Some(stdin) = self.tool.stdin.as_mut() {
            stdin.write_all(b"quit\n")?;
        }
        self.tool.wait()?;
        Ok(())
    }

    fn export(&mut self, collector: &Collector, timestamp: &Duration) -> anyhow::Result<()> {
        for proc in collector.lines() {
            let pid = proc.get_pid();
            if !self.pids.contains_key(&pid) {
                let filename = RrdExporter::filename(pid, proc.get_name());
                let start_time = timestamp
                    .checked_sub(self.interval)
                    .ok_or_else(|| Error::IntervalTooLarge)?;
                write!(
                    self.child_in,
                    "create {} --start={} --step={}",
                    filename,
                    start_time.as_secs(),
                    self.interval.as_secs()
                )?;
                for ds in &self.ds {
                    write!(self.child_in, " {}", ds)?;
                }
                write!(self.child_in, "RRA:AVERAGE:0.5:1:{}\n", self.size)?;
                self.read_answer()?;
                self.pids.insert(pid, filename);
            }
            let filename = self.pids.get(&pid).unwrap();
            write!(self.child_in, "update {} {}", filename, timestamp.as_secs())?;
            for (sample, skip) in proc.samples().zip(self.skip.iter()) {
                if !skip {
                    write!(self.child_in, ":{}", sample.values().next().unwrap())?;
                }
            }
            write!(self.child_in, "update {} {}", filename, timestamp.as_secs())?;
            self.read_answer()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {

    use std::io::{self, Seek, Write};

    use super::read_rrdtool_answer;

    #[test]
    fn parse_rrdtool_answer() -> anyhow::Result<()> {
        let mut bufok = io::Cursor::new(Vec::<u8>::new());
        write!(bufok, "OK u:0,01 s:0,02 r:8,05\n")?;
        bufok.seek(io::SeekFrom::Start(0)).unwrap();
        read_rrdtool_answer(&mut bufok)?;

        let mut buferr = io::Cursor::new(Vec::<u8>::new());
        write!(
            buferr,
            "ERROR: you must define at least one Round Robin Archive\n"
        )?;
        buferr.seek(io::SeekFrom::Start(0)).unwrap();
        match read_rrdtool_answer(&mut buferr) {
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
