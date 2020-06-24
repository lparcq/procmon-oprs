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

use anyhow::anyhow;
use libc::pid_t;
use log::info;
use std::collections::HashMap;
use std::iter::IntoIterator;
use std::time::Duration;

use crate::{
    agg::Aggregation,
    cfg::ExportSettings,
    collector::{Collector, ProcessStatus},
    metrics::MetricId,
};

use super::Exporter;

use crate::export::rrdtool::RrdTool;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("rrd: interval too large")]
    IntervalTooLarge,
    #[error("rrd: missing count")]
    MissingCount,
}

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
        | MetricId::IoReadTotal
        | MetricId::IoReadStorage
        | MetricId::IoWriteCall
        | MetricId::IoWriteTotal
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

struct ExportInfo {
    db: String,
    color: u32,
}

impl ExportInfo {
    fn new(db: String, color: u32) -> ExportInfo {
        ExportInfo { db, color }
    }
}

pub struct RrdExporter {
    interval: Duration,
    rows: usize,
    tool: RrdTool,
    ds: Vec<String>,
    skip: Vec<bool>,
    pids: HashMap<pid_t, ExportInfo>,
}

impl RrdExporter {
    pub fn new(settings: &ExportSettings, interval: Duration) -> anyhow::Result<RrdExporter> {
        let rows = settings.count.ok_or(Error::MissingCount)?;
        let tool = RrdTool::new(settings.dir.as_path())?;
        if interval.as_secs() == 0 || interval.subsec_nanos() != 0 {
            Err(anyhow!("rrd: interval must be a whole number of seconds"))
        } else {
            Ok(RrdExporter {
                interval,
                rows,
                tool,
                ds: Vec::new(),
                skip: Vec::new(),
                pids: HashMap::new(),
            })
        }
    }

    /// File name of a RRD.
    fn filename(pid: pid_t, name: &str) -> String {
        format!("{}_{}.rrd", name, pid)
    }

    /// Create process info.
    fn make_proc_info(&mut self, proc: &ProcessStatus, timestamp: &Duration) -> anyhow::Result<()> {
        let pid = proc.get_pid();
        let dbname = RrdExporter::filename(pid, proc.get_name());
        let start_time = timestamp
            .checked_sub(self.interval)
            .ok_or_else(|| Error::IntervalTooLarge)?;
        self.tool.create(
            &dbname,
            self.ds.iter(),
            &start_time,
            &self.interval,
            self.rows,
        )?;
        self.pids.insert(pid, ExportInfo::new(dbname, pid as u32));
        Ok(())
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
                let ds = format!("DS:{}:{}:{}:0.5:1", ds_name, ds_type, heart_beat,);
                info!("rrd define {}", ds);
                self.ds.push(ds);
            } else {
                self.skip.push(true);
            }
        });
        Ok(())
    }

    fn close(&mut self) -> anyhow::Result<()> {
        self.tool.close()
    }

    fn export(&mut self, collector: &Collector, timestamp: &Duration) -> anyhow::Result<()> {
        for pstat in collector.lines() {
            let pid = pstat.get_pid();
            let exinfo = match self.pids.get(&pid) {
                Some(exinfo) => exinfo,
                None => {
                    self.make_proc_info(pstat, timestamp)?;
                    self.pids.get(&pid).unwrap()
                }
            };

            let samples = pstat
                .samples()
                .into_iter()
                .zip(self.skip.iter())
                .filter(|(_, skip)| !*skip)
                .map(|(sample, _)| *(sample.values().next().unwrap()));
            self.tool.update(&exinfo.db, samples, timestamp)?;
        }
        Ok(())
    }
}
