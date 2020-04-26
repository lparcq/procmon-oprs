/// Extract metrics from procfs interface.
use procfs::process::{Io, Process, Stat, StatM};
use procfs::Meminfo;
use std::time::SystemTime;

use crate::metric::MetricId;

fn elapsed_time_since(start_time: u64) -> u64 {
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

/// System Configuration
pub struct SystemConf {
    ticks_per_second: u64,
    boot_time: u64,
    page_size: u64,
}

impl SystemConf {
    pub fn new() -> anyhow::Result<SystemConf> {
        let ticks_per_second = procfs::ticks_per_second()?;
        let kstat = procfs::KernelStats::new()?;
        let page_size = procfs::page_size()?;
        Ok(SystemConf {
            ticks_per_second: ticks_per_second as u64,
            boot_time: kstat.btime,
            page_size: page_size as u64,
        })
    }
}

/// System info
pub struct SystemInfo<'a> {
    system_conf: &'a SystemConf,
    meminfo: Option<Meminfo>,
}

impl<'a> SystemInfo<'a> {
    pub fn new(system_conf: &'a SystemConf) -> SystemInfo<'a> {
        SystemInfo {
            system_conf,
            meminfo: None,
        }
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

    pub fn extract_metrics(&mut self, ids: &[MetricId]) -> Vec<u64> {
        ids.iter()
            .map(|id| match id {
                MetricId::MemVm => {
                    self.with_meminfo(|mi| mi.mem_total - mi.mem_available.unwrap_or(mi.mem_free))
                }
                MetricId::TimeReal => elapsed_time_since(self.system_conf.boot_time),
                _ => 0,
            })
            .collect()
    }
}

/// Extract metrics for a process
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

    fn elapsed_time(stat: &Stat, system_conf: &SystemConf) -> u64 {
        let process_start = system_conf.boot_time + stat.starttime / system_conf.ticks_per_second;
        elapsed_time_since(process_start)
    }

    pub fn extract_metrics(&mut self, ids: &[MetricId]) -> Vec<u64> {
        ids.iter()
            .map(|id| match id {
                MetricId::FaultMinor => self.with_stat(|stat| stat.minflt),
                MetricId::FaultMajor => self.with_stat(|stat| stat.majflt),
                MetricId::IoReadCall => self.with_io(|io| io.rchar),
                MetricId::IoReadCount => self.with_io(|io| io.syscr),
                MetricId::IoReadStorage => self.with_io(|io| io.read_bytes),
                MetricId::IoWriteCall => self.with_io(|io| io.wchar),
                MetricId::IoWriteCount => self.with_io(|io| io.syscw),
                MetricId::IoWriteStorage => self.with_io(|io| io.write_bytes),
                MetricId::MemVm => self.with_stat(|stat| stat.vsize),
                MetricId::MemRss => {
                    self.with_stat(|stat| if stat.rss < 0 { 0 } else { stat.rss as u64 })
                }
                MetricId::MemText => self.with_system_statm(|statm, sc| statm.text * sc.page_size),
                MetricId::MemData => self.with_system_statm(|statm, sc| statm.data * sc.page_size),
                MetricId::TimeReal => self.with_system_stat(ProcessInfo::elapsed_time),
                MetricId::TimeSystem => {
                    self.with_stat(|stat| stat.stime) / self.system_conf.ticks_per_second
                }
                MetricId::TimeUser => {
                    self.with_stat(|stat| stat.utime) / self.system_conf.ticks_per_second
                }
            })
            .collect()
    }
}