// Oprs -- process monitor for Linux
// Copyright (C) 2020-2024 Laurent Pelecq
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

use libc::pid_t;
use std::{collections::HashMap, slice::Iter, time::SystemTime};

use procfs::{
    process::{FDTarget, Io, MMapPath, Stat, StatM},
    CpuTime, Current, CurrentSI, KernelStats, Meminfo, ProcResult,
};

pub use procfs::process::{Limit, LimitValue};

use super::Process;

use crate::metrics::{FormattedMetric, MetricId};

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

fn map_limit_value<F>(value: LimitValue, func: F) -> LimitValue
where
    F: Fn(u64) -> u64,
{
    match value {
        LimitValue::Value(value) => LimitValue::Value(func(value)),
        LimitValue::Unlimited => LimitValue::Unlimited,
    }
}

fn map_limit<F>(limit: Limit, func: F) -> Limit
where
    F: Fn(u64) -> u64,
{
    Limit {
        soft_limit: map_limit_value(limit.soft_limit, &func),
        hard_limit: map_limit_value(limit.hard_limit, &func),
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
        let ticks_per_second = procfs::ticks_per_second();
        let kstat = KernelStats::current()?;
        let page_size = procfs::page_size();

        Ok(SystemConf {
            ticks_per_second,
            boot_time_seconds: kstat.btime,
            page_size,
        })
    }

    /// Convert a number of ticks in milliseconds.
    /// A u64 can hold more than 10 millions years
    pub fn ticks_to_millis(&self, ticks: u64) -> u64 {
        ticks * 1000 / self.ticks_per_second
    }
}

/// System info
pub struct SystemStat<'a> {
    system_conf: &'a SystemConf,
    cputime: Option<CpuTime>,
    meminfo: Option<Meminfo>,
}

impl<'a> SystemStat<'a> {
    pub fn new(system_conf: &'a SystemConf) -> SystemStat<'a> {
        SystemStat {
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
            self.cputime = Some(
                KernelStats::current()
                    .expect("cannot access /proc/stat")
                    .total,
            );
        }
        self.cputime.as_ref().map_or(0, func)
    }

    fn with_meminfo<F>(&mut self, func: F) -> u64
    where
        F: Fn(&Meminfo) -> u64,
    {
        if self.meminfo.is_none() {
            self.meminfo = Some(Meminfo::current().expect("cannot access /proc/meminfo"));
        }
        match self.meminfo {
            Some(ref meminfo) => func(meminfo),
            None => panic!("internal error"),
        }
    }

    fn non_idle_ticks(&mut self) -> u64 {
        self.with_cputime(|ct| {
            (ct.user - ct.guest.unwrap_or(0))
                + (ct.nice - ct.guest_nice.unwrap_or(0))
                + ct.system
                + ct.iowait.unwrap_or(0)
                + ct.irq.unwrap_or(0)
                + ct.softirq.unwrap_or(0)
                + ct.steal.unwrap_or(0)
        })
    }

    pub fn total_time(&mut self) -> u64 {
        self.system_conf
            .ticks_to_millis(self.with_cputime(|ct| ct.idle) + self.non_idle_ticks())
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
                MetricId::TimeCpu => self.system_conf.ticks_to_millis(self.non_idle_ticks()),
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

/// Statistics about file descriptors
struct FdStats {
    highest: u32,                    // Highest file descriptor value
    total: usize,                    // Total number of file descriptors
    kinds: HashMap<MetricId, usize>, // Number of file descriptors by type
}

impl FdStats {
    fn new(process: &Process) -> ProcResult<FdStats> {
        let mut kinds = HashMap::new();
        kinds.insert(MetricId::FdAnon, 0);
        kinds.insert(MetricId::FdFile, 0);
        kinds.insert(MetricId::FdMemFile, 0);
        kinds.insert(MetricId::FdNet, 0);
        kinds.insert(MetricId::FdOther, 0);
        kinds.insert(MetricId::FdPipe, 0);
        kinds.insert(MetricId::FdSocket, 0);

        let fdinfos = process.fd()?;
        let mut highest = 0;
        let mut ninfos = 0;
        for fsres in fdinfos {
            let fdinfo = fsres?;
            ninfos += 1;
            if fdinfo.fd > highest {
                highest = fdinfo.fd;
            }
            let key = match fdinfo.target {
                FDTarget::AnonInode(_) => MetricId::FdAnon,
                FDTarget::MemFD(_) => MetricId::FdMemFile,
                FDTarget::Net(_) => MetricId::FdNet,
                FDTarget::Other(_, _) => MetricId::FdOther,
                FDTarget::Path(_) => MetricId::FdFile,
                FDTarget::Pipe(_) => MetricId::FdPipe,
                FDTarget::Socket(_) => MetricId::FdSocket,
            };
            if let Some(count_ref) = kinds.get_mut(&key) {
                *count_ref += 1
            }
        }
        Ok(FdStats {
            highest: highest as u32,
            total: ninfos,
            kinds,
        })
    }
}

/// Convert MMapPath to a metric for count
macro_rules! maps_count_key {
    ($mpath:expr) => {
        match $mpath {
            MMapPath::Path(_) => MetricId::MapFileCount,
            MMapPath::Heap => MetricId::MapHeapCount,
            MMapPath::Stack => MetricId::MapStackCount,
            MMapPath::TStack(_) => MetricId::MapThreadStackCount,
            MMapPath::Vdso => MetricId::MapVdsoCount,
            MMapPath::Vsys(_) => MetricId::MapVsysCount,
            MMapPath::Vvar => MetricId::MapVvarCount,
            MMapPath::Vsyscall => MetricId::MapVsyscallCount,
            MMapPath::Anonymous => MetricId::MapAnonCount,
            // Rollup is in smaps_rollup only. No need to have a metric when reading maps.
            MMapPath::Rollup | MMapPath::Other(_) => MetricId::MapOtherCount,
        }
    };
}

/// Convert MMapPath to a metric for size
macro_rules! maps_size_key {
    ($mpath:expr) => {
        match $mpath {
            MMapPath::Path(_) => MetricId::MapFileSize,
            MMapPath::Heap => MetricId::MapHeapSize,
            MMapPath::Stack => MetricId::MapStackSize,
            MMapPath::TStack(_) => MetricId::MapThreadStackSize,
            MMapPath::Vdso => MetricId::MapVdsoSize,
            MMapPath::Vsys(_) => MetricId::MapVsysSize,
            MMapPath::Vvar => MetricId::MapVvarSize,
            MMapPath::Vsyscall => MetricId::MapVsyscallSize,
            MMapPath::Anonymous => MetricId::MapAnonSize,
            // Rollup is in smaps_rollup only. No need to have a metric when reading maps.
            MMapPath::Rollup | MMapPath::Other(_) => MetricId::MapOtherSize,
        }
    };
}

struct MapsStats {
    counts: HashMap<MetricId, usize>,
    sizes: HashMap<MetricId, u64>,
}

impl MapsStats {
    fn new(process: &Process) -> ProcResult<MapsStats> {
        static COUNT_METRICS: [MetricId; 10] = [
            MetricId::MapAnonCount,
            MetricId::MapHeapCount,
            MetricId::MapFileCount,
            MetricId::MapStackCount,
            MetricId::MapThreadStackCount,
            MetricId::MapVdsoCount,
            MetricId::MapVsysCount,
            MetricId::MapVsyscallCount,
            MetricId::MapVvarCount,
            MetricId::MapOtherCount,
        ];
        static SIZE_METRICS: [MetricId; 10] = [
            MetricId::MapAnonSize,
            MetricId::MapHeapSize,
            MetricId::MapFileSize,
            MetricId::MapStackSize,
            MetricId::MapThreadStackSize,
            MetricId::MapVdsoSize,
            MetricId::MapVsysSize,
            MetricId::MapVsyscallSize,
            MetricId::MapVvarSize,
            MetricId::MapOtherSize,
        ];
        let mut counts = HashMap::new();
        COUNT_METRICS.iter().for_each(|id| {
            let _ = counts.insert(*id, 0usize);
        });
        let mut sizes = HashMap::new();
        SIZE_METRICS.iter().for_each(|id| {
            let _ = sizes.insert(*id, 0u64);
        });
        let maps = process.maps()?;
        maps.iter().for_each(|minfo| {
            if let Some(count_ref) = counts.get_mut(&maps_count_key!(minfo.pathname)) {
                *count_ref += 1;
            }
            if let Some(size_ref) = sizes.get_mut(&maps_size_key!(minfo.pathname)) {
                let (start, end) = minfo.address;
                *size_ref += end - start;
            }
        });
        Ok(MapsStats { counts, sizes })
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
pub struct ProcessStat<'a, 'b> {
    process: &'a Process,
    parent_pid: Option<pid_t>,
    system_conf: &'b SystemConf,
    fd_stats: Option<FdStats>,
    maps_stats: Option<MapsStats>,
    io: Option<Io>,
    stat: Option<Stat>,
    statm: Option<StatM>,
}

impl<'a, 'b> ProcessStat<'a, 'b> {
    pub fn new(process: &'a Process, system_conf: &'b SystemConf) -> ProcessStat<'a, 'b> {
        ProcessStat {
            process,
            parent_pid: None,
            system_conf,
            fd_stats: None,
            io: None,
            maps_stats: None,
            stat: None,
            statm: None,
        }
    }

    pub fn with_parent_pid(
        process: &'a Process,
        parent_pid: pid_t,
        system_conf: &'b SystemConf,
    ) -> ProcessStat<'a, 'b> {
        let mut pstat = ProcessStat::new(process, system_conf);
        pstat.parent_pid = Some(parent_pid);
        pstat
    }

    pub fn pid(&self) -> pid_t {
        self.process.pid()
    }

    pub fn parent_pid(&self) -> Option<pid_t> {
        self.parent_pid
            .or_else(|| self.stat.as_ref().map(|stat| stat.ppid))
    }

    fn with_fd_stats<F>(&mut self, func: F) -> u64
    where
        F: Fn(&FdStats) -> u64,
    {
        if self.fd_stats.is_none() {
            self.fd_stats = FdStats::new(self.process).ok();
        }
        self.fd_stats.as_ref().map_or(0, func)
    }

    fn with_io<F>(&mut self, func: F) -> u64
    where
        F: Fn(&Io) -> u64,
    {
        if self.io.is_none() {
            self.io = self.process.io().ok();
        }
        self.io.as_ref().map_or(0, func)
    }

    fn with_maps_stats<F>(&mut self, func: F) -> u64
    where
        F: Fn(&MapsStats) -> u64,
    {
        if self.maps_stats.is_none() {
            self.maps_stats = MapsStats::new(self.process).ok();
        }
        self.maps_stats.as_ref().map_or(0, func)
    }

    fn with_stat<F>(&mut self, func: F) -> u64
    where
        F: Fn(&Stat) -> u64,
    {
        if self.stat.is_none() {
            self.stat = self.process.stat().ok();
        }
        self.stat.as_ref().map_or(0, func)
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
                MetricId::FdAll => self.with_fd_stats(|stat| stat.total as u64),
                MetricId::FdHigh => self.with_fd_stats(|stat| stat.highest as u64),
                MetricId::FdAnon
                | MetricId::FdFile
                | MetricId::FdMemFile
                | MetricId::FdNet
                | MetricId::FdOther
                | MetricId::FdPipe
                | MetricId::FdSocket => self.with_fd_stats(|stat| stat.kinds[&metric.id] as u64),
                MetricId::IoReadCall => self.with_io(|io| io.rchar),
                MetricId::IoReadTotal => self.with_io(|io| io.syscr),
                MetricId::IoReadStorage => self.with_io(|io| io.read_bytes),
                MetricId::IoWriteCall => self.with_io(|io| io.wchar),
                MetricId::IoWriteTotal => self.with_io(|io| io.syscw),
                MetricId::IoWriteStorage => self.with_io(|io| io.write_bytes),
                MetricId::MapAnonCount
                | MetricId::MapHeapCount
                | MetricId::MapFileCount
                | MetricId::MapStackCount
                | MetricId::MapThreadStackCount
                | MetricId::MapVdsoCount
                | MetricId::MapVsysCount
                | MetricId::MapVsyscallCount
                | MetricId::MapVvarCount
                | MetricId::MapOtherCount => {
                    self.with_maps_stats(|stat| stat.counts[&metric.id] as u64)
                }
                MetricId::MapAnonSize
                | MetricId::MapHeapSize
                | MetricId::MapFileSize
                | MetricId::MapStackSize
                | MetricId::MapThreadStackSize
                | MetricId::MapVdsoSize
                | MetricId::MapVsysSize
                | MetricId::MapVsyscallSize
                | MetricId::MapVvarSize
                | MetricId::MapOtherSize => self.with_maps_stats(|stat| stat.sizes[&metric.id]),
                MetricId::MemVm => self.with_stat(|stat| stat.vsize),
                MetricId::MemRss => self.with_system_stat(|stat, sc| stat.rss * sc.page_size),
                MetricId::MemText => self.with_system_statm(|statm, sc| statm.text * sc.page_size),
                MetricId::MemData => self.with_system_statm(|statm, sc| statm.data * sc.page_size),
                MetricId::TimeElapsed => self.with_system_stat(ProcessStat::elapsed_seconds) * 1000,
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

    pub fn extract_limits(&mut self, metrics: Iter<FormattedMetric>) -> Vec<Option<Limit>> {
        match self.process.limits() {
            Ok(limits) => metrics
                .map(|metric| match metric.id {
                    MetricId::FdAll => Some(limits.max_open_files),
                    //MetricId::MemData => {} // max_data_size
                    MetricId::MapStackSize => Some(limits.max_stack_size),
                    MetricId::MemRss => Some(map_limit(limits.max_resident_set, |value| {
                        value * self.system_conf.page_size
                    })),
                    MetricId::MemVm => Some(limits.max_address_space),
                    MetricId::ThreadCount => Some(limits.max_processes),
                    MetricId::TimeCpu => Some(map_limit(limits.max_cpu_time, |value| value * 1000)),
                    _ => {
                        if cfg!(debug_assertions) && metric.has_limit() {
                            panic!("internal error: metric {} should have a limit", metric.id);
                        }
                        None
                    }
                })
                .collect(),
            Err(_) => vec![None; metrics.len()],
        }
    }
}
