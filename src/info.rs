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

// Extract metrics from procfs interface.

use procfs::{
    process::{Io, Process, Stat, StatM},
    KernelStats, Meminfo,
};
use std::path::PathBuf;
use std::slice::Iter;
use std::time::SystemTime;

use crate::{
    metrics::{FormattedMetric, MetricId},
    utils::read_file_first_line,
};

/// Elapsed time since a start time
/// Since the boot time is in seconds since the Epoch, no need to be more precise than the second.
fn elapsed_seconds_since(start_time: u64) -> u64 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => {
            let now = duration.as_secs();
            if now >= start_time {
                now - start_time
            } else {
                0
            }
        }
        Err(_) => 0,
    }
}

/// System CPU time.  
/// Replacement for procfs::KernelStats that returns time in number of seconds as f32 instead of ticks.
struct CpuTime {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: Option<u64>,
    pub irq: Option<u64>,
    pub softirq: Option<u64>,
    pub steal: Option<u64>,
    pub guest: Option<u64>,
    pub guest_nice: Option<u64>,
}

macro_rules! parse_u64 {
    ($lexer:expr) => {
        $lexer.next().map(|s| s.parse::<u64>()).transpose()
    };
    ($lexer:expr, $msg:expr) => {
        $lexer.next().expect($msg).parse::<u64>()
    };
}

impl CpuTime {
    fn new() -> anyhow::Result<CpuTime> {
        let line = read_file_first_line(PathBuf::from("/proc/stat"))?;
        CpuTime::parse(line.trim_end())
    }

    fn parse(line: &str) -> anyhow::Result<CpuTime> {
        let mut lexer = line.split_whitespace();
        assert!(lexer.next().expect("cannot parse /proc/stat") == "cpu");
        Ok(CpuTime {
            user: parse_u64!(lexer, "cannot parse user time in /proc/stat")?,
            nice: parse_u64!(lexer, "cannot parse user nice time in /proc/stat")?,
            system: parse_u64!(lexer, "cannot parse system time in /proc/stat")?,
            idle: parse_u64!(lexer, "cannot parse idle in /proc/stat")?,
            iowait: parse_u64!(lexer)?,
            irq: parse_u64!(lexer)?,
            softirq: parse_u64!(lexer)?,
            steal: parse_u64!(lexer)?,
            guest: parse_u64!(lexer)?,
            guest_nice: parse_u64!(lexer)?,
        })
    }
}

/// System Configuration
pub struct SystemConf {
    ticks_per_second: u64,
    boot_time_seconds: u64,
    page_size: u64,
}

impl SystemConf {
    pub fn new() -> anyhow::Result<SystemConf> {
        let ticks_per_second = procfs::ticks_per_second()?;
        let kstat = KernelStats::new()?;
        let page_size = procfs::page_size()?;
        Ok(SystemConf {
            ticks_per_second: ticks_per_second as u64,
            boot_time_seconds: kstat.btime,
            page_size: page_size as u64,
        })
    }

    /// Convert a number of ticks in milliseconds.
    /// A u64 can hold more than 10 millions years
    pub fn ticks_to_millis(&self, ticks: u64) -> u64 {
        ticks * 1000 / self.ticks_per_second
    }
}

/// System info
pub struct SystemInfo<'a> {
    system_conf: &'a SystemConf,
    cputime: Option<CpuTime>,
    meminfo: Option<Meminfo>,
}

impl<'a> SystemInfo<'a> {
    pub fn new(system_conf: &'a SystemConf) -> SystemInfo<'a> {
        SystemInfo {
            system_conf,
            cputime: None,
            meminfo: None,
        }
    }

    fn with_cputime<F>(&mut self, func: F) -> u64
    where
        F: Fn(&CpuTime) -> u64,
    {
        if self.cputime.is_none() {
            self.cputime = Some(CpuTime::new().expect("cannot access /proc/stat"));
        }
        self.cputime.as_ref().map_or(0, |cputime| func(cputime))
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

    pub fn extract_metrics(&mut self, metrics: Iter<FormattedMetric>) -> Vec<u64> {
        metrics
            .map(|metric| match metric.id {
                MetricId::MemVm => self
                    .with_meminfo(|mi| mi.mem_total - mi.mem_free + mi.swap_total - mi.swap_free),
                MetricId::MemRss => self.with_meminfo(|mi| mi.mem_total - mi.mem_free),
                MetricId::TimeElapsed => {
                    elapsed_seconds_since(self.system_conf.boot_time_seconds) * 1000
                }
                MetricId::TimeCpu => self.system_conf.ticks_to_millis(self.with_cputime(|ct| {
                    (ct.user - ct.guest.unwrap_or(0))
                        + (ct.nice - ct.guest_nice.unwrap_or(0))
                        + ct.system
                        + ct.idle
                        + ct.iowait.unwrap_or(0)
                        + ct.irq.unwrap_or(0)
                        + ct.softirq.unwrap_or(0)
                        + ct.steal.unwrap_or(0)
                })),
                MetricId::TimeSystem => self
                    .system_conf
                    .ticks_to_millis(self.with_cputime(|ct| ct.system)),
                MetricId::TimeUser => self
                    .system_conf
                    .ticks_to_millis(self.with_cputime(|ct| ct.user)),
                _ => 0,
            })
            .collect()
    }
}

/// Extract metrics for a process
///
/// Duration returned by the kernel are given in ticks. There are typically 100 ticks per
/// seconds. So the precision is 10ms. The duration are returned as a number of milliseconds.
///
/// Elapsed time is returned as a number of ticks since boot time. And boot time is given
/// as a number of seconds since the Epoch. Elapsed time is returned as milliseconds also
/// even if it's only precise in seconds.
pub struct ProcessInfo<'a, 'b> {
    process: &'a Process,
    system_conf: &'b SystemConf,
    io: Option<Io>,
    stat: Option<Stat>,
    statm: Option<StatM>,
}

impl<'a, 'b> ProcessInfo<'a, 'b> {
    pub fn new(process: &'a Process, system_conf: &'b SystemConf) -> ProcessInfo<'a, 'b> {
        ProcessInfo {
            process,
            system_conf,
            io: None,
            stat: None,
            statm: None,
        }
    }

    fn with_io<F>(&mut self, func: F) -> u64
    where
        F: Fn(&Io) -> u64,
    {
        if self.io.is_none() {
            self.io = self.process.io().ok();
        }
        self.io.as_ref().map_or(0, |io| func(io))
    }

    fn with_stat<F>(&mut self, func: F) -> u64
    where
        F: Fn(&Stat) -> u64,
    {
        if self.stat.is_none() {
            self.stat = self.process.stat().ok();
        }
        self.stat.as_ref().map_or(0, |stat| func(stat))
    }

    fn with_system_stat<F>(&mut self, func: F) -> u64
    where
        F: Fn(&Stat, &SystemConf) -> u64,
    {
        if self.stat.is_none() {
            self.stat = self.process.stat().ok();
        }
        self.stat
            .as_ref()
            .map_or(0, |stat| func(stat, self.system_conf))
    }

    fn with_system_statm<F>(&mut self, func: F) -> u64
    where
        F: Fn(&StatM, &SystemConf) -> u64,
    {
        if self.statm.is_none() {
            self.statm = self.process.statm().ok();
        }
        self.statm
            .as_ref()
            .map_or(0, |statm| func(statm, self.system_conf))
    }

    /// Elapsed seconds of the process
    fn elapsed_seconds(stat: &Stat, system_conf: &SystemConf) -> u64 {
        let process_start =
            system_conf.boot_time_seconds + stat.starttime / system_conf.ticks_per_second;
        elapsed_seconds_since(process_start)
    }

    pub fn extract_metrics(&mut self, metrics: Iter<FormattedMetric>) -> Vec<u64> {
        metrics
            .map(|metric| match metric.id {
                MetricId::FaultMinor => self.with_stat(|stat| stat.minflt),
                MetricId::FaultMajor => self.with_stat(|stat| stat.majflt),
                MetricId::IoReadCall => self.with_io(|io| io.rchar),
                MetricId::IoReadCount => self.with_io(|io| io.syscr),
                MetricId::IoReadStorage => self.with_io(|io| io.read_bytes),
                MetricId::IoWriteCall => self.with_io(|io| io.wchar),
                MetricId::IoWriteCount => self.with_io(|io| io.syscw),
                MetricId::IoWriteStorage => self.with_io(|io| io.write_bytes),
                MetricId::MemVm => self.with_stat(|stat| stat.vsize),
                MetricId::MemRss => self.with_system_stat(|stat, sc| {
                    if stat.rss < 0 {
                        0
                    } else {
                        (stat.rss as u64) * sc.page_size
                    }
                }),
                MetricId::MemText => self.with_system_statm(|statm, sc| statm.text * sc.page_size),
                MetricId::MemData => self.with_system_statm(|statm, sc| statm.data * sc.page_size),
                MetricId::TimeElapsed => self.with_system_stat(ProcessInfo::elapsed_seconds) * 1000,
                MetricId::TimeCpu => self
                    .system_conf
                    .ticks_to_millis(self.with_stat(|stat| stat.stime + stat.utime)),
                MetricId::TimeSystem => self
                    .system_conf
                    .ticks_to_millis(self.with_stat(|stat| stat.stime)),
                MetricId::TimeUser => self
                    .system_conf
                    .ticks_to_millis(self.with_stat(|stat| stat.utime)),
                MetricId::ThreadCount => self.with_stat(|stat| stat.num_threads as u64),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_cputime() {
        let ct =
            super::CpuTime::parse("cpu  236978 15 97017 6027274 1568 9614 7437 0 0 0").unwrap();
        assert_eq!(236978, ct.user);
        assert_eq!(15, ct.nice);
        assert_eq!(97017, ct.system);
        assert_eq!(6027274, ct.idle);
        assert_eq!(Some(1568), ct.iowait);
        assert_eq!(Some(9614), ct.irq);
        assert_eq!(Some(7437), ct.softirq);
        assert_eq!(Some(0), ct.steal);
        assert_eq!(Some(0), ct.guest);
        assert_eq!(Some(0), ct.guest_nice);
    }
}
