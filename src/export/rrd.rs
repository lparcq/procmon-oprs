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
use log::{debug, info};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use crate::{
    agg::Aggregation,
    cfg::ExportSettings,
    collector::{Collector, TargetStatus},
    metrics::MetricId,
};

use super::Exporter;

use crate::export::rrdtool::RrdTool;

/// Colors for graphs in order of priority (less used first).
const COLORS: [u32; 12] = [
    0xfa8072, // salmon
    0xcab2d6, // light purple
    0xffff55, // yellow
    0xb2df8a, // light green
    0xfb9a99, // pink
    0xa6cee3, // light blue
    0xb15928, // maroon
    0x6a3d9a, // purple
    0xff7f00, // orange
    0x33a02c, // green
    0xe31a1c, // red
    0x1f78b4, // blue
];

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("rrd: interval too large")]
    IntervalTooLarge,
    #[error("rrd: period too large (interval multiplied by rows)")]
    PeriodTooLarge,
    #[error("rrd: missing count")]
    MissingCount,
    #[error("rrd: number of colors exhausted")]
    NoMoreColors,
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
    name: String,
    db: String,
    color: u32,
}

impl ExportInfo {
    fn new(name: &str, db: &str, color: u32) -> ExportInfo {
        ExportInfo {
            name: name.to_string(),
            db: db.to_string(),
            color,
        }
    }
}

pub struct RrdExporter {
    interval: Duration,
    rows: usize,
    period: Duration,
    tool: RrdTool,
    variables: Vec<String>,
    ds: Vec<String>,
    skip: Vec<bool>,
    pids: HashMap<pid_t, Rc<ExportInfo>>,
    color_bucket: Vec<u32>,
    graph: bool,
}

impl RrdExporter {
    pub fn new(settings: &ExportSettings, interval: Duration) -> anyhow::Result<RrdExporter> {
        let rows = settings.count.ok_or(Error::MissingCount)?;
        let tool = RrdTool::new(settings.dir.as_path())?;
        let period = interval
            .checked_mul(rows as u32)
            .ok_or(Error::PeriodTooLarge)?;
        if interval.as_secs() == 0 || interval.subsec_nanos() != 0 {
            Err(anyhow!("rrd: interval must be a whole number of seconds"))
        } else {
            Ok(RrdExporter {
                interval,
                rows,
                period,
                tool,
                ds: Vec::new(),
                variables: Vec::new(),
                skip: Vec::new(),
                pids: HashMap::new(),
                color_bucket: Vec::from(COLORS),
                graph: settings.graph,
            })
        }
    }

    /// File name of a RRD.
    fn filename(pid: pid_t, name: &str) -> String {
        format!("{}_{}.rrd", name, pid)
    }

    /// Create process info.
    fn insert_export_info(
        &mut self,
        status: &TargetStatus,
        timestamp: &Duration,
    ) -> anyhow::Result<()> {
        let pid = status.get_pid();
        let dbname = RrdExporter::filename(pid, status.get_name());
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
        let color = if self.graph {
            self.color_bucket.pop().ok_or(Error::NoMoreColors)?
        } else {
            0
        };
        let exinfo = Rc::new(ExportInfo::new(status.get_name(), &dbname, color));
        self.pids.insert(pid, exinfo);
        Ok(())
    }
}

impl Exporter for RrdExporter {
    fn open(&mut self, collector: &Collector) -> anyhow::Result<()> {
        let heart_beat = self.interval.as_secs() * 2;
        collector.for_each_computed_metric(|id, agg| {
            let ds_name = id.as_str().replace(":", "_");
            let ds_type = match data_source_type(id) {
                DataSourceType::Counter => "COUNTER",
                DataSourceType::Gauge => "GAUGE",
            };
            if let Aggregation::None = agg {
                self.skip.push(false);
                let ds = format!("DS:{}:{}:{}:0:U", &ds_name, ds_type, heart_beat,);
                self.variables.push(ds_name);
                info!("rrd define {}", ds);
                self.ds.push(ds);
            } else {
                self.skip.push(true);
            }
        });
        Ok(())
    }

    fn close(&mut self) -> anyhow::Result<()> {
        self.tool.close()?;
        Ok(())
    }

    fn export(&mut self, collector: &Collector, timestamp: &Duration) -> anyhow::Result<()> {
        let mut pids: HashSet<pid_t> = self.pids.keys().copied().collect();
        let mut infos = Vec::new();
        for status in collector.lines() {
            let pid = status.get_pid();
            if pid == 0 {
                continue;
            }
            if !pids.remove(&pid) {
                self.insert_export_info(status, timestamp)?;
            }
            let exinfo = self.pids.get(&pid).unwrap();
            if self.graph {
                infos.push(exinfo.clone());
            }

            let samples = status
                .samples()
                .zip(self.skip.iter())
                .filter(|(_, skip)| !*skip)
                .map(|(sample, _)| *(sample.values().next().unwrap()));
            self.tool.update(&exinfo.db, samples, timestamp)?;
        }
        if self.graph {
            let start = timestamp
                .checked_sub(self.period)
                .ok_or(Error::PeriodTooLarge)?;
            for ds_name in &self.variables {
                let title = ds_name.replace("_", " ");
                let filename = format!("{}.png", ds_name);
                let defs = infos.iter().enumerate().map(|(index, exinfo)| {
                    let def = format!(
                        "DEF:v{}={}:{}:AVERAGE LINE1:v{}#{:0>6x}:\"{}\"",
                        index, exinfo.db, ds_name, index, exinfo.color, exinfo.name
                    );
                    debug!("rrd def: {}", def);
                    def
                });
                let (width, height) =
                    self.tool
                        .graph(&filename, &start, timestamp, defs, Some(&title))?;
                debug!("graph of size ({}, {})", width, height);
            }
        }
        for pid in pids {
            if let Some(exinfo) = self.pids.remove(&pid) {
                self.color_bucket.push(exinfo.color);
            }
        }
        Ok(())
    }
}
