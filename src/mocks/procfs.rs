pub(crate) use std::io::Error as ProcError;

pub(crate) type ProcResult<T> = Result<T, ProcError>;

pub(crate) mod process {

    use libc::pid_t;
    use procfs::process::{FDInfo, Io, Limits, MemoryMaps, Stat, StatM};
    use std::{cell::RefCell, io, path::PathBuf};

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

    #[derive(Debug)]
    pub(crate) struct Process {
        pid: pid_t,
        parent_pid: pid_t,
        exe: Option<String>,
        path: Option<PathBuf>,
        start_time: u64,
        ttl: Option<RefCell<u16>>,
    }

    impl Process {
        pub(crate) fn new(pid: pid_t) -> ProcResult<Self> {
            Ok(Self {
                pid,
                parent_pid: 0,
                exe: None,
                path: None,
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
            let path = PathBuf::from(exe);
            Self {
                pid,
                parent_pid,
                exe: Some(exe.to_string()),
                path: Some(path),
                start_time,
                ttl: ttl.map(RefCell::new),
            }
        }

        pub(crate) fn is_alive(&self) -> bool {
            match &self.ttl {
                Some(ttl) if *ttl.borrow() == 0 => false,
                _ => self.exe.is_some(),
            }
        }

        pub(crate) fn decrease_ttl(&self) {
            if let Some(ref ttl) = self.ttl {
                let mut ttl = ttl.borrow_mut();
                if let Some(value) = ttl.checked_sub(1) {
                    *ttl = value;
                }
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
            match self.path.as_ref() {
                Some(path) => {
                    let mut st: Stat = procfs::FromRead::from_read(io::Cursor::new(format!(
                        "{} ({}) S {} {}",
                        self.pid,
                        path.file_name()
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
                None => Err(new_error("no stat")),
            }
        }

        pub(crate) fn statm(&self) -> ProcResult<StatM> {
            Err(new_error("Process::statm not implemented"))
        }
    }
}
