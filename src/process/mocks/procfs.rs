// Oprs -- process monitor for Linux
// Copyright (C) 2024-2025 Laurent Pelecq
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

pub(crate) use std::io::Error as ProcError;

pub(crate) type ProcResult<T> = Result<T, ProcError>;

#[derive(Clone, Debug, Default)]
pub(crate) struct CpuTime {
    utime: u64,
    stime: u64,
}

impl CpuTime {
    fn new(utime: u64, stime: u64) -> Self {
        Self { utime, stime }
    }
}

pub(crate) mod process {

    use libc::pid_t;
    use procfs::process::{FDInfo, Io, Limits, MemoryMaps, StatM};
    use std::{cell::RefCell, collections::HashMap, ffi::OsString, io, path::PathBuf, rc::Rc};

    pub(crate) use procfs::process::Stat;

    use super::{CpuTime, ProcError, ProcResult};

    #[derive(Debug)]
    pub(crate) struct FDsIter {}

    impl std::iter::Iterator for FDsIter {
        type Item = ProcResult<FDInfo>;
        fn next(&mut self) -> Option<ProcResult<FDInfo>> {
            None
        }
    }

    fn new_error(msg: &str) -> ProcError {
        io::Error::new(io::ErrorKind::Other, msg)
    }

    #[derive(Debug, Clone)]
    pub(crate) struct Process {
        pid: pid_t,
        parent_pid: Rc<RefCell<pid_t>>,
        exe: Option<String>,
        start_time: u64,
        cpu_time: Rc<RefCell<CpuTime>>,
        ttl: Option<Rc<RefCell<u16>>>,
    }

    impl Process {
        pub(crate) fn new_fake(
            pid: pid_t,
            parent_pid: pid_t,
            exe: Option<&str>,
            start_time: u64,
            cpu_time: CpuTime,
            ttl: Option<u16>,
        ) -> Self {
            Self {
                pid,
                parent_pid: Rc::new(RefCell::new(parent_pid)),
                exe: exe.map(str::to_string),
                start_time,
                cpu_time: Rc::new(RefCell::new(cpu_time)),
                ttl: ttl.map(RefCell::new).map(Rc::new),
            }
        }

        pub(crate) fn new(pid: pid_t) -> ProcResult<Self> {
            let cpu_time = CpuTime::default();
            Ok(Self::new_fake(pid, 0, None, 0, cpu_time, None))
        }

        pub(crate) fn reparent(&mut self, parent_pid: pid_t) {
            *self.parent_pid.borrow_mut() = parent_pid;
        }

        fn check_if_alive(&self) -> bool {
            match self.ttl {
                Some(ref ttl) => {
                    let mut ttl = ttl.borrow_mut();
                    ttl.checked_sub(1)
                        .map(|value| {
                            *ttl = value;
                            true
                        })
                        .unwrap_or(false)
                }
                None => self.exe.is_some(),
            }
        }

        pub(crate) fn is_alive(&self) -> bool {
            match self.ttl {
                Some(ref ttl) => *ttl.borrow() > 0,
                None => self.exe.is_some(),
            }
        }

        pub(crate) fn ttl(&self) -> Option<u16> {
            self.ttl.as_ref().map(|ttl| *ttl.borrow())
        }

        pub(crate) fn set_ttl(&mut self, new_ttl: u16) {
            match self.ttl {
                Some(ref ttl) => {
                    let mut ttl = ttl.borrow_mut();
                    *ttl = new_ttl;
                }
                None => self.ttl = Some(Rc::new(RefCell::new(new_ttl))),
            }
        }

        pub(crate) fn cmdline(&self) -> ProcResult<Vec<String>> {
            self.exe
                .as_ref()
                .map(|exe| vec![exe.to_string()])
                .ok_or_else(|| new_error("no command line"))
        }

        pub(crate) fn uid(&self) -> ProcResult<u32> {
            Ok(0)
        }

        pub(crate) fn exe(&self) -> ProcResult<PathBuf> {
            self.exe
                .as_ref()
                .map(PathBuf::from)
                .ok_or_else(|| new_error("no executable"))
        }

        pub(crate) fn cwd(&self) -> ProcResult<PathBuf> {
            Err(new_error("Process::cwd not implemented"))
        }

        pub(crate) fn fd(&self) -> ProcResult<FDsIter> {
            Err(new_error("Process::fd not implemented"))
        }

        pub(crate) fn io(&self) -> ProcResult<Io> {
            Err(new_error("Process::io not implemented"))
        }

        pub(crate) fn environ(&self) -> ProcResult<HashMap<OsString, OsString>> {
            Err(new_error("Process::environ not implemented"))
        }

        pub(crate) fn limits(&self) -> ProcResult<Limits> {
            Err(new_error("Process::limits not implemented"))
        }

        pub(crate) fn maps(&self) -> ProcResult<MemoryMaps> {
            Err(new_error("Process::maps not implemented"))
        }

        pub(crate) fn pid(&self) -> pid_t {
            self.pid
        }

        pub(crate) fn stat(&self) -> ProcResult<Stat> {
            if self.check_if_alive() {
                let cpu_time = self.cpu_time.borrow();
                let mut st: Stat = procfs::FromRead::from_read(io::Cursor::new(format!(
                    "{} ({}) S {} {}",
                    self.pid,
                    self.exe()?
                        .file_name()
                        .expect("Process::stat: exe has no file name")
                        .to_str()
                        .expect("Process::stat: unprintable file name"),
                    self.parent_pid.borrow(),
                    (0..50)
                        .map(|i| i.to_string())
                        .collect::<Vec<String>>()
                        .join(" "),
                )))
                .expect("Process::stat: cannot decode fake stat");
                st.starttime = self.start_time;
                st.utime = cpu_time.utime;
                st.stime = cpu_time.stime;
                Ok(st)
            } else {
                Err(new_error("Process died"))
            }
        }

        pub(crate) fn statm(&self) -> ProcResult<StatM> {
            Err(new_error("Process::statm not implemented"))
        }

        /// Simulate CPU.
        pub(crate) fn schedule(&self, utime: u64, stime: u64) {
            let mut cpu_time = self.cpu_time.borrow_mut();
            cpu_time.utime += utime;
            cpu_time.stime += stime;
        }
    }

    #[derive(Debug)]
    pub(crate) struct ProcessIter {}

    impl std::iter::Iterator for ProcessIter {
        type Item = ProcResult<Process>;
        fn next(&mut self) -> Option<ProcResult<Process>> {
            None
        }
    }

    pub(crate) fn all_processes() -> ProcResult<ProcessIter> {
        Err(new_error("all_processes not implemented"))
    }
}

use libc::pid_t;
use std::{sync::LazyLock, time::Instant};

use process::Process;

static ORIGIN: LazyLock<Instant> = LazyLock::new(Instant::now);

#[derive(Debug)]
pub(crate) struct ProcessBuilder {
    pid: pid_t,
    parent_pid: pid_t,
    name: String,
    start_time: u64,
    cpu_time: CpuTime,
    ttl: Option<u16>,
}

impl ProcessBuilder {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            pid: 0,
            parent_pid: 0,
            name: name.to_string(),
            start_time: ORIGIN.elapsed().as_nanos() as u64,
            cpu_time: CpuTime::default(),
            ttl: None,
        }
    }

    pub(crate) fn pid(mut self, pid: pid_t) -> Self {
        self.pid = pid;
        self
    }

    pub(crate) fn parent_pid(mut self, parent_pid: pid_t) -> Self {
        self.parent_pid = parent_pid;
        self
    }

    pub(crate) fn name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    pub(crate) fn start_time(mut self, start_time: u64) -> Self {
        self.start_time = start_time;
        self
    }

    pub(crate) fn cpu_time(mut self, utime: u64, stime: u64) -> Self {
        self.cpu_time = CpuTime::new(utime, stime);
        self
    }

    pub(crate) fn ttl(mut self, ttl: u16) -> Self {
        self.ttl = Some(ttl);
        self
    }

    pub(crate) fn build(self) -> Process {
        let exe = format!("/bin/{}", self.name);
        Process::new_fake(
            self.pid,
            self.parent_pid,
            Some(&exe),
            self.start_time,
            self.cpu_time,
            self.ttl,
        )
    }
}
