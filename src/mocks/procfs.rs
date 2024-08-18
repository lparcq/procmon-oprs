pub(crate) use std::io::Error as ProcError;

pub(crate) type ProcResult<T> = Result<T, ProcError>;

pub(crate) mod process {

    use libc::pid_t;
    use procfs::process::{FDInfo, Io, Limits, MemoryMaps, Stat, StatM};
    use std::{cell::RefCell, io, path::PathBuf, rc::Rc};

    use super::{ProcError, ProcResult};

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
        parent_pid: pid_t,
        exe: Option<String>,
        start_time: u64,
        ttl: Option<Rc<RefCell<u16>>>,
    }

    impl Process {
        pub(crate) fn new(pid: pid_t) -> ProcResult<Self> {
            Ok(Self {
                pid,
                parent_pid: 0,
                exe: None,
                start_time: 0,
                ttl: None,
            })
        }

        pub(crate) fn with_exe(
            pid: pid_t,
            parent_pid: pid_t,
            exe: &str,
            start_time: u64,
            ttl: Option<u16>,
        ) -> Self {
            Self {
                pid,
                parent_pid,
                exe: Some(exe.to_string()),
                start_time,
                ttl: ttl.map(RefCell::new).map(Rc::new),
            }
        }

        pub(crate) fn is_alive(&self) -> bool {
            match self.ttl {
                Some(ref ttl) => {
                    let mut ttl = ttl.borrow_mut();
                    match ttl.checked_sub(1) {
                        Some(value) => {
                            *ttl = value;
                            value > 0
                        }
                        None => false,
                    }
                }
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

        pub(crate) fn exe(&self) -> ProcResult<PathBuf> {
            self.exe
                .as_ref()
                .map(|exe| PathBuf::from(exe))
                .ok_or_else(|| new_error("no executable"))
        }

        pub(crate) fn fd(&self) -> ProcResult<FDsIter> {
            Err(new_error("Process::fd not implemented"))
        }

        pub(crate) fn io(&self) -> ProcResult<Io> {
            Err(new_error("Process::io not implemented"))
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

        pub(crate) fn starttime(&self) -> u64 {
            self.start_time
        }

        pub(crate) fn stat(&self) -> ProcResult<Stat> {
            let mut st: Stat = procfs::FromRead::from_read(io::Cursor::new(format!(
                "{} ({}) S {} {}",
                self.pid,
                self.exe()?
                    .file_name()
                    .expect("Process::stat: exe has no file name")
                    .to_str()
                    .expect("Process::stat: unprintable file name"),
                self.parent_pid,
                (0..50)
                    .map(|i| i.to_string())
                    .collect::<Vec<String>>()
                    .join(" "),
                //self.start_time
            )))
            .expect("Process::stat: cannot decode fake stat");
            st.starttime = self.start_time;

            Ok(st)
        }

        pub(crate) fn statm(&self) -> ProcResult<StatM> {
            Err(new_error("Process::statm not implemented"))
        }
    }

    /// Return the same process with a different parent.
    pub(crate) fn reparent_process(proc: &Process, parent_pid: pid_t) -> Process {
        Process {
            pid: proc.pid,
            parent_pid,
            exe: proc.exe.clone(),
            start_time: proc.start_time,
            ttl: proc.ttl.as_ref().map(Rc::clone),
        }
    }
}

use libc::pid_t;
use std::{sync::LazyLock, time::Instant};

use process::Process;

pub(crate) use process::reparent_process;

static ORIGIN: LazyLock<Instant> = LazyLock::new(|| Instant::now());

#[derive(Debug)]
pub(crate) struct ProcessBuilder {
    pid: pid_t,
    parent_pid: pid_t,
    name: String,
    start_time: u64,
    ttl: Option<u16>,
}

impl ProcessBuilder {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            pid: 0,
            parent_pid: 0,
            start_time: ORIGIN.elapsed().as_nanos() as u64,
            name: name.to_string(),
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

    pub(crate) fn ttl(mut self, ttl: u16) -> Self {
        self.ttl = Some(ttl);
        self
    }

    pub(crate) fn build(self) -> Process {
        let exe = format!("/bin/{}", self.name);
        Process::with_exe(self.pid, self.parent_pid, &exe, self.start_time, self.ttl)
    }
}
