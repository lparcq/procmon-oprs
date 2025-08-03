// Oprs -- process monitor for Linux
// Copyright (C) 2020-2025 Laurent Pelecq
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

use std::{collections::HashMap, fmt, slice::Iter, sync::OnceLock, time::SystemTime};

use procfs::{
    process::{FDTarget, Io, MMapPath, Stat, StatM},
    CpuInfo, CpuTime, Current, CurrentSI, KernelStats, Meminfo, ProcResult,
};

use super::{FormattedMetric, MetricId, Process};

#[derive(thiserror::Error, Debug)]
pub enum StatError {
    #[error("cannot get kernel statistics: {0}")]
    KernelStats(String),
}

pub type StatResult<T> = Result<T, StatError>;

/// Elapsed time since a start time
/// Since the boot time is in seconds since the Epoch, no need to be more precise than the second.
fn elapsed_seconds_since(start_time: u64) -> u64 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => duration.as_secs().saturating_sub(start_time),
        Err(_) => 0,
    }
}

/// System Configuration
#[derive(Debug, Clone, Copy)]
pub struct SystemConf {
    ticks_per_second: u64,
    boot_time_seconds: u64,
    page_size: u64,
}

pub static SYS_CONF: OnceLock<SystemConf> = OnceLock::new();

impl SystemConf {
    pub fn initialize() -> StatResult<&'static SystemConf> {
        let ticks_per_second = procfs::ticks_per_second();
        let kstat =
            KernelStats::current().map_err(|err| StatError::KernelStats(format!("{err:?}")))?;
        let page_size = procfs::page_size();
        Ok(SYS_CONF.get_or_init(|| SystemConf {
            ticks_per_second,
            boot_time_seconds: kstat.btime,
            page_size,
        }))
    }

    /// Convert a number of ticks in milliseconds.
    /// A u64 can hold more than 10 millions years
    pub fn ticks_to_millis(&self, ticks: u64) -> u64 {
        ticks * 1000 / self.ticks_per_second
    }
}

macro_rules! sysconf {
    () => {
        SYS_CONF.get().expect("system configuration")
    };
}

macro_rules! ticks_to_millis {
    ($ticks:expr) => {
        sysconf!().ticks_to_millis($ticks)
    };
}

/// System info
#[derive(Debug)]
pub struct SystemStat {
    cputime: Option<CpuTime>,
    meminfo: Option<Meminfo>,
}

impl SystemStat {
    pub fn new() -> SystemStat {
        SystemStat {
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
        sysconf!().ticks_to_millis(self.with_cputime(|ct| ct.idle) + self.non_idle_ticks())
    }

    pub fn extract_metrics(&mut self, metrics: Iter<FormattedMetric>) -> Vec<u64> {
        metrics
            .map(|metric| match metric.id {
                MetricId::MemVm => self
                    .with_meminfo(|mi| mi.mem_total - mi.mem_free + mi.swap_total - mi.swap_free),
                MetricId::MemRss => self.with_meminfo(|mi| mi.mem_total - mi.mem_free),
                MetricId::TimeElapsed => elapsed_seconds_since(sysconf!().boot_time_seconds) * 1000,
                MetricId::TimeCpu => ticks_to_millis!(self.non_idle_ticks()),
                MetricId::TimeSystem => ticks_to_millis!(self.with_cputime(|ct| ct.system)),
                MetricId::TimeUser => ticks_to_millis!(self.with_cputime(|ct| ct.user)),
                _ => 0,
            })
            .collect()
    }

    /// Number of cores
    pub fn num_cores() -> Option<usize> {
        CpuInfo::current().ok().as_ref().map(CpuInfo::num_cores)
    }

    /// RAM size
    pub fn mem_total() -> Option<u64> {
        Meminfo::current().ok().map(|m| m.mem_total)
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
#[derive(Default)]
pub struct ProcessStat {
    fd_stats: Option<FdStats>,
    maps_stats: Option<MapsStats>,
    io: Option<Io>,
    stat: Option<Stat>,
    statm: Option<StatM>,
}

impl ProcessStat {
    pub fn with_stat(stat: Stat) -> Self {
        ProcessStat {
            fd_stats: None,
            io: None,
            maps_stats: None,
            stat: Some(stat),
            statm: None,
        }
    }

    fn on_optional_stat<F, T>(&mut self, process: &Process, func: F) -> Option<T>
    where
        F: Fn(&Stat) -> T,
    {
        if self.stat.is_none() {
            self.stat = process.stat().ok();
        }
        self.stat.as_ref().map(func)
    }

    fn on_fd_stats<F>(&mut self, process: &Process, func: F) -> u64
    where
        F: Fn(&FdStats) -> u64,
    {
        if self.fd_stats.is_none() {
            self.fd_stats = FdStats::new(process).ok();
        }
        self.fd_stats.as_ref().map_or(0, func)
    }

    fn on_io<F>(&mut self, process: &Process, func: F) -> u64
    where
        F: Fn(&Io) -> u64,
    {
        if self.io.is_none() {
            self.io = process.io().ok();
        }
        self.io.as_ref().map_or(0, func)
    }

    fn on_maps_stats<F>(&mut self, process: &Process, func: F) -> u64
    where
        F: Fn(&MapsStats) -> u64,
    {
        if self.maps_stats.is_none() {
            self.maps_stats = MapsStats::new(process).ok();
        }
        self.maps_stats.as_ref().map_or(0, func)
    }

    fn on_stat<F>(&mut self, process: &Process, func: F) -> u64
    where
        F: Fn(&Stat) -> u64,
    {
        self.on_optional_stat(process, func).unwrap_or(0)
    }

    fn on_system_stat<F>(&mut self, process: &Process, func: F) -> u64
    where
        F: Fn(&Stat) -> u64,
    {
        if self.stat.is_none() {
            self.stat = process.stat().ok();
        }
        self.stat.as_ref().map_or(0, func)
    }

    fn on_system_statm<F>(&mut self, process: &Process, func: F) -> u64
    where
        F: Fn(&StatM) -> u64,
    {
        if self.statm.is_none() {
            self.statm = process.statm().ok();
        }
        self.statm.as_ref().map_or(0, func)
    }

    /// Elapsed seconds of the process
    fn elapsed_seconds(stat: &Stat) -> u64 {
        let sysconf = sysconf!();
        let process_start = sysconf.boot_time_seconds + stat.starttime / sysconf.ticks_per_second;
        elapsed_seconds_since(process_start)
    }

    pub fn extract_metrics(
        &mut self,
        metrics: Iter<FormattedMetric>,
        process: &Process,
    ) -> Vec<u64> {
        metrics
            .map(|metric| match metric.id {
                MetricId::FaultMinor => self.on_stat(process, |stat| stat.minflt),
                MetricId::FaultMajor => self.on_stat(process, |stat| stat.majflt),
                MetricId::FdAll => self.on_fd_stats(process, |stat| stat.total as u64),
                MetricId::FdHigh => self.on_fd_stats(process, |stat| stat.highest as u64),
                MetricId::FdAnon
                | MetricId::FdFile
                | MetricId::FdMemFile
                | MetricId::FdNet
                | MetricId::FdOther
                | MetricId::FdPipe
                | MetricId::FdSocket => {
                    self.on_fd_stats(process, |stat| stat.kinds[&metric.id] as u64)
                }
                MetricId::IoReadCall => self.on_io(process, |io| io.rchar),
                MetricId::IoReadTotal => self.on_io(process, |io| io.syscr),
                MetricId::IoReadStorage => self.on_io(process, |io| io.read_bytes),
                MetricId::IoWriteCall => self.on_io(process, |io| io.wchar),
                MetricId::IoWriteTotal => self.on_io(process, |io| io.syscw),
                MetricId::IoWriteStorage => self.on_io(process, |io| io.write_bytes),
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
                    self.on_maps_stats(process, |stat| stat.counts[&metric.id] as u64)
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
                | MetricId::MapOtherSize => {
                    self.on_maps_stats(process, |stat| stat.sizes[&metric.id])
                }
                MetricId::MemVm => self.on_stat(process, |stat| stat.vsize),
                MetricId::MemRss => {
                    self.on_system_stat(process, |stat| stat.rss * sysconf!().page_size)
                }
                MetricId::MemText => {
                    self.on_system_statm(process, |statm| statm.text * sysconf!().page_size)
                }
                MetricId::MemData => {
                    self.on_system_statm(process, |statm| statm.data * sysconf!().page_size)
                }
                MetricId::TimeElapsed => {
                    self.on_system_stat(process, ProcessStat::elapsed_seconds) * 1000
                }
                MetricId::TimeCpu => {
                    ticks_to_millis!(self.on_stat(process, |stat| stat.stime + stat.utime))
                }
                MetricId::TimeSystem => {
                    ticks_to_millis!(self.on_stat(process, |stat| stat.stime))
                }
                MetricId::TimeUser => {
                    ticks_to_millis!(self.on_stat(process, |stat| stat.utime))
                }
                MetricId::ThreadCount => self.on_stat(process, |stat| stat.num_threads as u64),
            })
            .collect()
    }
}

macro_rules! anonymous_option {
    ($opt:expr) => {
        match $opt {
            Some(_) => &"Some(_)",
            None => &"None",
        }
    };
}

impl fmt::Debug for ProcessStat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("ProcessStat")
            .field("fd_stats", anonymous_option!(self.fd_stats))
            .field("maps_stats", anonymous_option!(self.maps_stats))
            .field("io", anonymous_option!(self.io))
            .field("stat", anonymous_option!(self.stat))
            .field("statm", anonymous_option!(self.statm))
            .finish()
    }
}
