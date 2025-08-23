// Oprs -- process monitor for Linux
// Copyright (C) 2024-2025  Laurent Pelecq
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

use getset::{CopyGetters, Getters};
use indextree::{Arena, NodeId};
use libc::pid_t;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    iter::Iterator,
    path::PathBuf,
    slice::Iter,
};

use super::{FormattedMetric, ProcessStat};

#[cfg(not(test))]
pub use procfs::{
    ProcResult,
    process::{self, Process, all_processes},
};

#[cfg(test)]
pub(crate) use super::mocks::procfs::{
    self, ProcResult,
    process::{self, Process, all_processes},
};

#[cfg(feature = "tui")]
fn format_path(path: PathBuf) -> String {
    path.to_str()
        .map(String::from)
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

#[cfg(all(not(test), feature = "tui"))]
mod format {
    use super::format_path;
    use procfs::ProcError;

    pub(crate) fn format_process_error(err: ProcError) -> String {
        let msg = match err {
            ProcError::PermissionDenied(_) => "permission denied",
            ProcError::NotFound(_) => "not found",
            ProcError::Incomplete(_) => "incomplete",
            _ => "unknown error",
        };
        match err {
            ProcError::PermissionDenied(None)
            | ProcError::NotFound(None)
            | ProcError::Incomplete(None) => msg.to_string(),
            ProcError::PermissionDenied(Some(path))
            | ProcError::NotFound(Some(path))
            | ProcError::Incomplete(Some(path)) => format!("{}: {}", format_path(path), msg),
            ProcError::Io(err, None) => err.to_string(),
            ProcError::Io(err, Some(path)) => format!("{}: {}", format_path(path), err),
            ProcError::Other(err) => err,
            ProcError::InternalError(err) => err.to_string(),
        }
    }
}

#[cfg(all(test, feature = "tui"))]
mod format {
    use super::super::mocks::procfs::ProcError;

    pub(crate) fn format_process_error(err: ProcError) -> String {
        format!("{err:?}")
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ProcessError {
    #[error("unknown process {0}")]
    UnknownProcess(pid_t),
    #[error("cannot access processes")]
    CannotAccessProcesses,
}

pub type ProcessResult<T> = Result<T, ProcessError>;

/// Format a result returned by procfs.
#[cfg(feature = "tui")]
pub fn format_result(res: ProcResult<PathBuf>) -> String {
    match res {
        Ok(path) => format_path(path),
        Err(err) => format::format_process_error(err),
    }
}

/// Executable name
///
/// Based of the first element of the command line if it exists or the name of
/// the executable.
fn exe_name(process: &Process) -> Option<String> {
    process
        .cmdline()
        .map(|c| c.first().map(PathBuf::from))
        .ok()
        .flatten()
        .or_else(|| process.exe().ok())
        .and_then(|path| {
            path.file_name()
                .and_then(|os_name| os_name.to_str())
                .map(|s| s.to_string())
        })
}

fn new_stat(process: &Process) -> ProcessResult<process::Stat> {
    process
        .stat()
        .map_err(|_| ProcessError::UnknownProcess(process.pid()))
}

/// Record CPU activity.
#[derive(Debug, Default)]
struct CpuActivity {
    cpu_time: u64,
    idleness: u16,
}

impl CpuActivity {
    /// Return 1 if no CPU has been used or 0
    fn update(&mut self, stat: &process::Stat) {
        let cpu_time = stat.utime.saturating_add(stat.stime);
        if cpu_time > self.cpu_time {
            self.cpu_time = cpu_time;
            self.idleness = 0;
        } else {
            self.idleness = self.idleness.saturating_add(1);
        }
    }
}

#[derive(Debug, Getters, CopyGetters)]
/// Information about for an existing or past process.
pub struct ProcessInfo {
    /// Process identifier.
    #[getset(get_copy = "pub")]
    pid: pid_t,
    /// Parent process identifier.
    #[getset(get_copy = "pub")]
    parent_pid: pid_t,
    /// Process creation time.
    start_time: u64,
    /// Process state
    #[getset(get_copy = "pub")]
    state: char,
    /// Process name.
    #[getset(get = "pub")]
    name: String,
    /// Process instance.
    #[getset(get = "pub")]
    process: Process,
    /// Process statistics.
    stats: RefCell<ProcessStat>,
    /// Whether this is a kernel process.
    ///
    /// Assuming that processes without command line and exe is a kernel process.
    /// On Linux, kernel processes are children of process 2.
    #[getset(get_copy = "pub")]
    is_kernel: bool,
    /// Process exists but is hidden.
    #[getset(get_copy = "pub")]
    hidden: bool,
    /// Activity of the process.
    activity: RefCell<CpuActivity>,
}

impl ProcessInfo {
    fn new(process: Process) -> ProcessResult<Self> {
        let pid = process.pid();
        let stat = new_stat(&process)?;
        let parent_pid = stat.ppid;
        let start_time = stat.starttime;
        let state = stat.state;
        let exe_name = exe_name(&process);
        let is_kernel = exe_name.is_none();
        let name = exe_name.unwrap_or_else(|| format!("({})", stat.comm));
        let mut activity = CpuActivity::default();
        activity.update(&stat);
        let stats = RefCell::new(ProcessStat::with_stat(stat));
        Ok(Self {
            pid,
            parent_pid,
            start_time,
            state,
            name,
            process,
            stats,
            is_kernel,
            hidden: true,
            activity: RefCell::new(activity),
        })
    }

    pub fn with_pid(pid: pid_t) -> ProcessResult<Self> {
        let process = Process::new(pid).map_err(|_| ProcessError::UnknownProcess(pid))?;
        Self::new(process)
    }

    #[cfg(feature = "tui")]
    pub fn uid(&self) -> Option<u32> {
        self.process.uid().ok()
    }

    #[cfg(feature = "tui")]
    pub fn cmdline(&self) -> String {
        self.process
            .cmdline()
            .map(|v| v.join(" "))
            .unwrap_or_else(|_| String::from("<zombie>"))
    }

    pub fn hide(&mut self) {
        self.hidden = true;
    }

    pub fn show(&mut self) {
        self.hidden = false;
    }

    pub fn idleness(&self) -> u16 {
        self.activity.borrow().idleness
    }

    pub fn refresh(&mut self) -> ProcessResult<()> {
        let stat = new_stat(&self.process)?;
        if stat.starttime != self.start_time {
            // Not the same process. PID has been reused
            Err(ProcessError::UnknownProcess(self.pid))
        } else {
            self.parent_pid = stat.ppid;
            self.activity.borrow_mut().update(&stat);
            self.stats = RefCell::new(ProcessStat::with_stat(stat));
            Ok(())
        }
    }

    pub fn extract_metrics(&self, metrics: Iter<FormattedMetric>) -> Vec<u64> {
        self.stats
            .borrow_mut()
            .extract_metrics(metrics, &self.process)
    }
}

/// Iterator on a forest roots.
pub struct RootIter<'a, 'b> {
    forest: &'a Forest,
    inner: std::collections::btree_set::Iter<'b, NodeId>,
}

impl<'a> Iterator for RootIter<'a, '_> {
    type Item = &'a ProcessInfo;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|node_id| self.forest.get_known_info(*node_id))
    }
}

/// Iterator on a process descendants.
pub struct Descendants<'a, 'b> {
    forest: &'a Forest,
    inner: indextree::Descendants<'b, ProcessInfo>,
}

impl<'a> Iterator for Descendants<'a, '_> {
    type Item = &'a ProcessInfo;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|node_id| self.forest.get_known_info(node_id))
    }
}

/// Trait to implement filters.
pub trait ProcessClassifier {
    fn accept(&self, pi: &ProcessInfo) -> bool;
}

/// Classifier that accepts all processes.
#[derive(Debug, Default)]
struct AcceptAllProcesses(());

impl ProcessClassifier for AcceptAllProcesses {
    fn accept(&self, _pi: &ProcessInfo) -> bool {
        true
    }
}

#[derive(Debug)]
/// State used during refresh
struct RefreshState {
    /// Processes that are not selected but may be the parent of other processes.
    candidates: BTreeMap<pid_t, ProcessInfo>,
    /// Set of nodes to remove at the end of the refresh.
    old_nodes: BTreeSet<NodeId>,
    /// The forest is changed if there are new processes or processes that die.
    changed: bool,
}

impl RefreshState {
    fn new(arena: &Arena<ProcessInfo>) -> Self {
        Self {
            candidates: BTreeMap::new(),
            old_nodes: BTreeSet::from_iter(arena.iter().filter_map(|node| {
                if node.is_removed() {
                    None
                } else {
                    Some(arena.get_node_id(node).unwrap())
                }
            })),
            changed: false,
        }
    }

    fn remove_old_node(&mut self, node_id: &NodeId) {
        let _ = self.old_nodes.remove(node_id);
        self.changed = true;
    }
}

/// Forest of processes.
///
/// There may be multiple roots. All processes matching a predicate plus their ancestors
/// are in the forest.
pub struct Forest {
    arena: Arena<ProcessInfo>,
    roots: BTreeSet<NodeId>,
    processes: BTreeMap<pid_t, NodeId>,
}

impl Forest {
    pub fn new() -> Self {
        Self {
            arena: Arena::new(),
            roots: BTreeSet::new(),
            processes: BTreeMap::new(),
        }
    }

    /// Get a process that is known to be in the arena.
    fn get_known_info(&self, node_id: NodeId) -> &ProcessInfo {
        self.arena
            .get(node_id)
            .expect("Internal error: dangling root in tree.")
            .get()
    }

    /// List of root node ids that are children of a given PID.
    fn adopted_roots(&self, pid: pid_t) -> Vec<NodeId> {
        self.roots
            .iter()
            .filter(|root_id| self.get_known_info(**root_id).parent_pid() == pid)
            .copied()
            .collect()
    }

    /// Add a process in the tree.
    ///
    /// A process is useful if it is shown or if it is the parent of another
    /// process in the tree.
    ///
    /// # Arguments
    ///
    /// * `state` - The refrest state.
    /// * `info` - The new process info.
    /// * `adopted_ids` - List of node ids of roots that are children of the new node.
    fn add_node_internal(
        &mut self,
        state: &mut RefreshState,
        info: ProcessInfo,
        adopted_ids: &[NodeId],
    ) {
        let pid = info.pid();
        let hidden = info.hidden();
        let parent_pid = info.parent_pid();
        let node_id = self.arena.new_node(info);
        if !hidden {
            state.remove_old_node(&node_id);
        }
        log::debug!("indextree[{}]: new_node {node_id}", std::line!());
        self.processes.insert(pid, node_id);
        // It may be a parent of a root
        for root_id in adopted_ids {
            self.roots.remove(root_id);
            log::debug!("indextree[{}]: {node_id}.append({root_id})", std::line!(),);
            node_id.append(*root_id, &mut self.arena);
        }
        if let Some(parent_info) = state.candidates.remove(&parent_pid) {
            self.add_node(state, parent_info);
        }
        match self.processes.get(&parent_pid) {
            Some(parent_node_id) => {
                parent_node_id
                    .ancestors(&self.arena)
                    .for_each(|node_id| state.remove_old_node(&node_id));
                log::debug!(
                    "indextree[{}]: {parent_node_id}.append({node_id})",
                    std::line!(),
                );
                parent_node_id.append(node_id, &mut self.arena);
            }
            None => {
                self.roots.insert(node_id); // This node is a root
            }
        }
        state.changed = true;
    }

    /// Add a node in the tree.
    fn add_node(&mut self, state: &mut RefreshState, info: ProcessInfo) {
        let pid = info.pid();
        let adopted_ids = self.adopted_roots(pid);
        self.add_node_internal(state, info, &adopted_ids);
    }

    /// Add a process in the tree if it is not hidden or a parent of another process.
    fn add_useful_node(
        &mut self,
        state: &mut RefreshState,
        info: ProcessInfo,
    ) -> Option<ProcessInfo> {
        let pid = info.pid();
        let adopted_ids = self.adopted_roots(pid);
        let hidden = info.hidden();
        if hidden && adopted_ids.is_empty() {
            // This process is useless.
            Some(info)
        } else {
            self.add_node_internal(state, info, &adopted_ids);
            None
        }
    }

    /// Remove a node if it exists.
    fn remove_node(&mut self, state: &mut RefreshState, node_id: NodeId, reason: &'static str) {
        if let Some(node) = self.arena.get(node_id) {
            if !node.is_removed() {
                let pid = node.get().pid();
                self.processes.remove(&pid);
                log::debug!("indextree[{}]: {node_id}.remove(): {reason}", std::line!());
                node_id.remove(&mut self.arena);
            }
            self.roots.remove(&node_id);
            state.remove_old_node(&node_id);
        }
    }

    /// reparent a node if parent PID has changed.
    fn reparent_node(&mut self, new_parent_pid: pid_t, node_id: &NodeId) {
        if let Some(node) = self.arena.get(*node_id)
            && let Some(parent_node_id) = node.parent()
            && let Some(parent_pid) = self.arena.get(parent_node_id).map(|node| node.get().pid())
            && parent_pid != new_parent_pid
        {
            node_id.detach(&mut self.arena);
            match self.processes.get(&new_parent_pid) {
                Some(parent_node_id) => parent_node_id.append(*node_id, &mut self.arena),
                None => {
                    log::error!("parent {new_parent_pid} should have been already in the tree")
                }
            }
        }
    }

    /// Remove a node and its children.
    fn remove_subtree(&mut self, state: &mut RefreshState, node_id: NodeId) {
        let child_node_ids = node_id.children(&self.arena).collect::<Vec<NodeId>>();
        for child_id in child_node_ids {
            self.remove_subtree(state, child_id);
        }
        self.remove_node(state, node_id, "subtree");
    }

    /// Remove subtrees.
    fn remove_subtrees(&mut self, state: &mut RefreshState) {
        while let Some(node_id) = state.old_nodes.first() {
            self.remove_subtree(state, *node_id);
        }
    }

    /// Number of processes
    #[cfg(test)]
    pub fn size(&self) -> usize {
        self.processes.len()
    }

    #[cfg(test)]
    pub fn get_process(&self, pid: pid_t) -> Option<&ProcessInfo> {
        // Get process with a given PID if it exists.
        self.processes
            .get(&pid)
            .map(|node_id| self.get_known_info(*node_id))
    }

    pub fn has_process(&self, pid: pid_t) -> bool {
        self.processes.contains_key(&pid)
    }

    // Remove a process that doesn't exists.
    //
    // The children are moved on the parent.
    fn remove_non_existing_pid(&mut self, pid: pid_t) {
        if let Some(node_id) = self.processes.remove(&pid) {
            log::debug!("indextree[{}]: {node_id}.remove()", std::line!());
            // Children are reparented.
            if self.roots.remove(&node_id) {
                // It was a root. Children become roots.
                node_id.children(&self.arena).for_each(|node_id| {
                    let _ = self.roots.insert(node_id);
                });
            }
            node_id.remove(&mut self.arena);
        }
    }

    /// Iterate roots
    pub fn iter_roots<'a: 'b, 'b>(&'a self) -> RootIter<'a, 'b> {
        RootIter {
            forest: self,
            inner: self.roots.iter(),
        }
    }

    /// Descendants of a pid
    ///
    /// Include the root process itself.
    pub fn descendants(&self, pid: pid_t) -> ProcessResult<Descendants<'_, '_>> {
        match self.processes.get(&pid) {
            Some(node_id) => Ok(Descendants {
                forest: self,
                inner: node_id.descendants(&self.arena),
            }),
            None => Err(ProcessError::UnknownProcess(pid)),
        }
    }

    /// Root PIDs
    pub fn root_pids(&self) -> Vec<pid_t> {
        self.iter_roots().map(|p| p.pid()).collect::<Vec<pid_t>>()
    }

    /// Iterate on all processes and apply the conditional function
    pub fn filter_collect<V, F>(&self, func: F) -> Vec<V>
    where
        F: Fn(&ProcessInfo) -> Option<V>,
    {
        let mut result = Vec::new();
        self.iter_roots().for_each(|p| {
            if let Ok(descendants) = self.descendants(p.pid()) {
                descendants.for_each(|p| {
                    if let Some(v) = func(p) {
                        result.push(v)
                    }
                })
            }
        });
        result
    }

    /// Refresh existing processes.
    ///
    /// Refresh the stats and hide all processes.
    fn refresh_existing_processes(&mut self) {
        let mut invalid_pids = Vec::new();
        self.arena.iter_mut().for_each(|node| {
            if !node.is_removed() {
                let info = node.get_mut();
                match info.refresh() {
                    Ok(()) => info.hide(),
                    Err(_) => invalid_pids.push(info.pid()),
                }
            }
        });
        for pid in invalid_pids {
            log::debug!("{pid}: cannot access stat file");
            self.remove_non_existing_pid(pid);
        }
    }

    /// Refreshes the forest and return if it has changed.
    pub fn refresh_from<I, C>(&mut self, processes: I, classifier: &C) -> bool
    where
        I: Iterator<Item = Process>,
        C: ProcessClassifier,
    {
        log::debug!("refresh");
        self.refresh_existing_processes();
        let mut state = RefreshState::new(&self.arena);
        for process in processes {
            let pid = process.pid();
            match self.processes.get(&pid).copied() {
                Some(node_id) => {
                    let (shown, parent_id) = match self.arena.get_mut(node_id) {
                        // Existing process
                        Some(node) => {
                            let info = node.get_mut();
                            let shown = classifier.accept(info);
                            if shown {
                                info.show();
                            }
                            (shown, info.parent_pid())
                        }
                        None => panic!("inconsistency between PID index and the tree"),
                    };
                    if shown {
                        self.reparent_node(parent_id, &node_id);
                        node_id
                            .ancestors(&self.arena)
                            .for_each(|node_id| state.remove_old_node(&node_id));
                    }
                }
                None => {
                    // New process
                    match ProcessInfo::new(process) {
                        Ok(mut info) => {
                            if classifier.accept(&info) {
                                info.show();
                            }
                            if let Some(info) = self.add_useful_node(&mut state, info) {
                                state.candidates.insert(pid, info);
                            }
                        }
                        Err(err) => log::error!("{pid}: {err:?}"),
                    }
                }
            }
        }
        self.remove_subtrees(&mut state);
        state.changed
    }

    /// Refresh the forest with all the visible processes in the system if they match the predicate.
    pub fn refresh_if<C>(&mut self, classifier: &C) -> Result<bool, ProcessError>
    where
        C: ProcessClassifier,
    {
        Ok(self.refresh_from(
            all_processes()
                .map_err(|_| ProcessError::CannotAccessProcesses)?
                .filter_map(ProcResult::ok),
            classifier,
        ))
    }

    /// Refresh the forest with all the visible processes in the system.
    pub fn refresh(&mut self) -> Result<bool, ProcessError> {
        self.refresh_if(&AcceptAllProcesses::default())
    }
}

#[cfg(test)]
mod tests {

    use rand::seq::SliceRandom;
    use std::{
        collections::{BTreeSet, HashMap},
        iter::IntoIterator,
    };

    use super::{
        AcceptAllProcesses, Forest, Process, ProcessClassifier, ProcessInfo, pid_t,
        procfs::ProcessBuilder,
    };

    fn sorted<T, I>(input: I) -> Vec<T>
    where
        T: Clone + Ord,
        I: IntoIterator<Item = T>,
    {
        let mut v = input.into_iter().collect::<Vec<T>>();
        v.sort();
        v
    }

    fn shuffle(mut processes: Vec<Process>) -> Vec<Process> {
        processes.shuffle(&mut rand::rng());
        processes
    }

    /// Accept a specific processes.
    #[derive(Debug)]
    struct AcceptProcesses(BTreeSet<pid_t>);

    impl AcceptProcesses {
        fn with_pids(pids: &[pid_t]) -> Self {
            AcceptProcesses(BTreeSet::from_iter(pids.iter().copied()))
        }

        fn with_pid(pid: pid_t) -> Self {
            AcceptProcesses::with_pids(&[pid])
        }

        fn contains(&self, pid: pid_t) -> bool {
            let Self(pids) = self;
            pids.contains(&pid)
        }
    }

    impl ProcessClassifier for AcceptProcesses {
        fn accept(&self, pi: &ProcessInfo) -> bool {
            self.contains(pi.pid)
        }
    }

    #[derive(Debug, Default)]
    struct ProcessFactory {
        pid: pid_t,
        count: usize,
    }

    impl ProcessFactory {
        /// Return a builder with predefined name and pid and parent pid is the last pid.
        fn builder_with_pid(&mut self, pid: pid_t) -> ProcessBuilder {
            let name = self.next_name();
            let parent_pid = self.pid;
            if self.pid < pid {
                self.pid = pid;
            }
            ProcessBuilder::new(&name)
                .pid(self.pid)
                .parent_pid(parent_pid)
        }

        /// Return a default builder with predefined name and parent pid is the last pid.
        fn builder(&mut self) -> ProcessBuilder {
            self.builder_with_pid(self.pid + 1)
        }

        /// Build a process with default parameters.
        fn build(&mut self) -> Process {
            self.builder().build()
        }

        /// Next unique name.
        fn next_name(&mut self) -> String {
            let count = self.count;
            self.count += 1;
            format!("proc{count}")
        }

        /// Last PID used.
        fn last_pid(&self) -> pid_t {
            self.pid
        }

        /// Builds a forest based on constraits on parent pids.
        ///
        /// The processes with pid from 0 to `count` are added in the forest.
        ///
        /// The first process has no parent. By default, process parent is the last process.
        ///
        /// Ex: [ (2, Some(0)), (3, None) ] means that the parent of process #2 is
        /// process #0 (the root) and that process #3 has no parent. It describes a
        /// forest of two trees.
        fn with_parent_pids(
            &mut self,
            constraints: &[(usize, Option<usize>)],
            count: usize,
        ) -> Vec<Process> {
            let constraints =
                HashMap::<usize, Option<usize>>::from_iter(constraints.iter().copied());
            let mut pids = Vec::new();
            (0..count)
                .map(|idx| {
                    let parent_pid = constraints
                        .get(&idx)
                        .map(|opt_idx| opt_idx.map(|idx| pids[idx]).unwrap_or(0))
                        .unwrap_or(self.last_pid());
                    let proc = self.builder().parent_pid(parent_pid).build();
                    pids.push(proc.pid());
                    proc
                })
                .collect::<Vec<Process>>()
        }
    }

    #[test]
    /// Make sure the factory builds correct processes.
    ///
    /// |_0
    /// | |_1_2_3
    /// | \_4_5
    /// |_6
    ///   \_7
    fn test_forest_from_parent_pids() {
        let mut factory = ProcessFactory::default();
        let parent_pids = &[0, 1, 2, 3, 1, 5, 0, 7];
        let constraints = &[(1, Some(0)), (4, Some(0)), (6, None)];
        let processes = factory.with_parent_pids(constraints, 8);
        assert_eq!(8, processes.len());
        for (expected_ppid, proc) in std::iter::zip(parent_pids, processes) {
            let parent_pid = proc.stat().unwrap().ppid;
            assert_eq!(*expected_ppid, parent_pid);
        }
    }

    #[test]
    fn test_process_stat() {
        const PID: pid_t = 10;
        const PARENT_PID: pid_t = 8;
        const START_TIME: u64 = 1234;
        const NAME: &str = "fake";
        let proc = ProcessFactory::default()
            .builder()
            .pid(PID)
            .parent_pid(PARENT_PID)
            .name(NAME)
            .start_time(START_TIME)
            .build();
        let st = proc.stat().unwrap();
        assert_eq!(PID, st.pid);
        assert_eq!(PARENT_PID, st.ppid);
        assert_eq!(NAME, st.comm);
        assert_eq!(START_TIME, st.starttime);
    }

    #[test]
    /// Create an empty forest.
    fn test_empty() {
        let mut forest = Forest::new();
        let mut empty: Vec<Process> = Vec::new();
        forest.refresh_from(empty.drain(..), &AcceptProcesses::with_pid(1));
    }

    #[test]
    /// Create a forest with one process.
    fn test_one_process() {
        const NAME: &str = "test";
        let mut factory = ProcessFactory::default();
        let mut forest = Forest::new();
        let mut processes = vec![factory.builder().name(NAME).build()];
        let first_pid = factory.last_pid();
        forest.refresh_from(processes.drain(..), &AcceptProcesses::with_pid(first_pid));
        let pinfo = forest.get_process(first_pid).unwrap();
        assert_eq!(first_pid, pinfo.pid());
        assert_eq!(first_pid, pinfo.process().pid());
        assert_eq!(NAME, pinfo.name());
        assert!(forest.has_process(first_pid));
    }

    #[test]
    /// Test TTL.
    fn test_ttl() {
        const TTL: u16 = 2;
        const NAME: &str = "test";
        let mut factory = ProcessFactory::default();
        let mut forest = Forest::new();
        let processes = vec![factory.builder().name(NAME).ttl(TTL).build()];
        let first_pid = factory.last_pid();
        let any_proc = AcceptAllProcesses::default();
        for _ in 0..TTL {
            forest.refresh_from(processes.clone().drain(..), &any_proc);
            assert!(forest.get_process(first_pid).is_some());
        }
        forest.refresh_from(processes.clone().drain(..), &any_proc);
        assert!(forest.get_process(first_pid).is_none());
    }

    #[test]
    /// Test idleness.
    ///
    /// The idleness increases when the CPU time is null. It drops to zero otherwise.
    fn test_idleness() {
        const NAME: &str = "test";
        let mut factory = ProcessFactory::default();

        // Create an idle process.
        let proc1 = factory.builder().name(NAME).build();
        let pinfo1 = ProcessInfo::new(proc1).unwrap();
        assert!(pinfo1.idleness() > 0);

        // Create a process with user time.
        let proc2 = factory.builder().name(NAME).cpu_time(1234, 0).build();
        let pinfo2 = ProcessInfo::new(proc2).unwrap();
        assert_eq!(0, pinfo2.idleness());

        // Create a process with system time.
        let proc3 = factory.builder().name(NAME).cpu_time(0, 1234).build();
        let pinfo3 = ProcessInfo::new(proc3).unwrap();
        assert_eq!(0, pinfo3.idleness());

        // One process in the forest.
        let any_proc = AcceptAllProcesses::default();
        let mut forest = Forest::new();
        let proc = factory.builder().name(NAME).build();
        let pid = proc.pid();
        let processes = vec![proc.clone()];
        forest.refresh_from(processes.clone().drain(..), &any_proc);
        assert_eq!(1, forest.get_process(pid).unwrap().idleness());

        forest.refresh_from(processes.clone().drain(..), &any_proc);
        assert_eq!(2, forest.get_process(pid).unwrap().idleness());

        proc.schedule(1234, 0);
        forest.refresh_from(processes.clone().drain(..), &any_proc);
        assert_eq!(0, forest.get_process(pid).unwrap().idleness());

        forest.refresh_from(processes.clone().drain(..), &any_proc);
        assert_eq!(1, forest.get_process(pid).unwrap().idleness());
    }

    /// Create a forest with a single tree.
    ///
    /// Create a process tree but insert them in the list in a different order so
    /// that children comes sometimes before sometimes after their parent.
    ///
    /// Tree:
    /// 0
    /// |_1_2
    /// |   \_5
    /// \_3_4
    #[test]
    fn test_single_tree() {
        let mut factory = ProcessFactory::default();
        let mut processes = factory.with_parent_pids(&[(3, Some(0)), (5, Some(2))], 6);

        let mut forest = Forest::new();
        forest.refresh_from(processes.drain(..), &AcceptAllProcesses::default());
        let root_pids = forest.root_pids();
        assert_eq!(vec![1], root_pids);

        let expected_exe_tree = vec!["proc0", "proc1", "proc2", "proc5", "proc3", "proc4"];
        let exe_tree = forest
            .descendants(root_pids[0])
            .unwrap()
            .map(|p| p.name().to_string())
            .collect::<Vec<String>>();
        assert_eq!(expected_exe_tree.len(), exe_tree.len());
        std::iter::zip(expected_exe_tree, exe_tree).for_each(|(expected_name, name)| {
            assert_eq!(expected_name, name);
        });
    }

    #[test]
    /// Build a forest of two trees and check that there are two roots.
    fn test_multi_trees() {
        let mut factory = ProcessFactory::default();
        let mut processes = shuffle(factory.with_parent_pids(&[(4, None)], 8));

        let mut forest = Forest::new();
        forest.refresh_from(processes.drain(..), &AcceptAllProcesses::default());

        let expected_pids = vec![1, 5];
        let pids = sorted(forest.root_pids());
        assert_eq!(expected_pids, pids);
    }

    #[test]
    /// Build a tree with a predicate.
    ///
    /// - Unselected process must be hidden.
    /// - Selected process must not be hidden.
    /// - Only selected processes and their parents are in the tree.
    ///
    /// Tree:
    /// 0
    /// |_1_2_3
    /// |   \_[4]
    /// \_5_[6]_7
    ///
    /// - Processes 5 and 7 are selected.
    /// - Processes 4 and 8 must not be in the tree.
    /// - Other processes are hidden.
    fn test_predicate() {
        let mut factory = ProcessFactory::default();
        let mut processes = factory.with_parent_pids(&[(4, Some(2)), (5, Some(0))], 8);
        let proc3_pid = processes[3].pid();
        let proc4_pid = processes[4].pid();
        let proc6_pid = processes[6].pid();
        let proc7_pid = processes[7].pid();

        let mut forest = Forest::new();
        let classifier = AcceptProcesses::with_pids(&[proc4_pid, proc6_pid]);
        forest.refresh_from(processes.drain(..), &classifier);

        let root_pid = forest.root_pids()[0];

        assert_eq!(6, forest.size()); // Process 3 and 7 are discarded
        for pinfo in forest.descendants(root_pid).unwrap() {
            assert_eq!(classifier.accept(pinfo), !pinfo.hidden());
            // Processes that have been discarded
            assert_ne!(proc3_pid, pinfo.pid());
            assert_ne!(proc7_pid, pinfo.pid());
        }
    }

    #[test]
    /// Refresh multiple times with different predicates.
    ///
    /// Tree:
    /// 0
    /// |_1_2_3
    /// |   \_4
    /// \_5_6_7
    fn test_refresh_different_predicates() {
        pub fn test_predicate<I>(
            forest: &mut Forest,
            stage: &str,
            processes: I,
            all_pids: &[pid_t],
            selected_pids: &[pid_t],
        ) where
            I: Iterator<Item = Process>,
        {
            let classifier = AcceptProcesses::with_pids(selected_pids);
            forest.refresh_from(processes, &classifier);
            for (n, pid) in all_pids.iter().enumerate() {
                let is_selected = classifier.contains(*pid);
                let is_hidden = match forest.get_process(*pid) {
                    Some(p) => p.hidden(),
                    None => true,
                };
                assert_eq!(
                    is_selected, !is_hidden,
                    "{stage}: process #{n} pid:{pid} selected:{is_selected} hidden:{is_hidden}"
                );
            }
        }

        let mut factory = ProcessFactory::default();
        let processes1 = factory.with_parent_pids(&[(4, Some(2)), (5, Some(0))], 8);
        let processes2 = processes1.clone();
        let pids = processes1.iter().map(Process::pid).collect::<Vec<pid_t>>();

        let mut forest = Forest::new();
        test_predicate(
            &mut forest,
            "loop1",
            shuffle(processes1).drain(..),
            &pids,
            &[pids[2], pids[3], pids[6], pids[7]],
        );
        test_predicate(
            &mut forest,
            "loop2",
            shuffle(processes2).drain(..),
            &pids,
            &[pids[3], pids[6], pids[7]],
        );
    }

    #[test]
    /// Refresh a tree with new processes.
    fn test_refresh_with_new_processes() {
        let mut factory = ProcessFactory::default();
        let root = factory.build();
        let root_pid = root.pid();
        let mut processes = Vec::new();
        processes.push(root);

        let any_proc = AcceptAllProcesses::default();
        let mut forest = Forest::new();
        forest.refresh_from(processes.clone().drain(..), &any_proc);
        for count in 2..6 {
            // Add a new process at each loop.
            let proc = factory.builder().parent_pid(root_pid).build();
            processes.push(proc);
            forest.refresh_from(shuffle(processes.clone()).drain(..), &any_proc);
            assert_eq!(count, forest.size());
        }
    }

    #[test]
    /// Refresh a tree with processes that die.
    ///
    /// Tree:
    /// 0
    /// |_1_2_5
    /// \_3_4
    fn test_refresh_with_old_processes() {
        let mut factory = ProcessFactory::default();
        let mut processes1 = factory.with_parent_pids(&[(3, Some(0))], 5);
        let proc2_pid = processes1[2].pid();
        let mut processes2 = processes1.clone();

        let any_proc = AcceptAllProcesses::default();
        let mut forest = Forest::new();
        forest.refresh_from(processes1.drain(..), &any_proc);
        assert_eq!(5, forest.size());

        let mut ttl = 3;
        let proc = factory.builder().parent_pid(proc2_pid).ttl(ttl).build();
        let proc_pid = proc.pid();
        assert_eq!(ttl, proc.ttl().unwrap());
        processes2.push(proc);

        loop {
            ttl = ttl.saturating_sub(1);
            forest.refresh_from(processes2.clone().drain(..), &any_proc);
            match forest.get_process(proc_pid) {
                Some(info) => assert_eq!(ttl, info.process().ttl().unwrap()),
                None => break,
            }
            assert_eq!(6, forest.size());
        }
        assert!(forest.get_process(proc_pid).is_none());

        let mut processes3 = processes2
            .iter()
            .filter(|proc| proc.pid() != proc_pid)
            .cloned()
            .collect::<Vec<Process>>();
        forest.refresh_from(processes3.drain(..), &any_proc);
        assert_eq!(5, forest.size());
    }

    #[test]
    /// Refresh a tree where the root process dies.
    ///
    /// Tree:
    /// 0
    /// |_1_2
    /// \_3_4
    fn test_refresh_with_root_stopped() {
        let mut factory = ProcessFactory::default();
        let mut processes1 = factory.with_parent_pids(&[(3, Some(0))], 5);
        let root = &mut processes1[0];
        root.set_ttl(1);
        let root = root.clone();
        let root_pid = root.pid();
        let proc1_pid = processes1[1].pid();
        let proc3_pid = processes1[3].pid();
        let mut processes2 = processes1.clone();

        let any_proc = AcceptAllProcesses::default();
        let mut forest = Forest::new();
        forest.refresh_from(shuffle(processes1).drain(..), &any_proc);
        assert_eq!(5, forest.size());
        assert_eq!(vec![root_pid], forest.root_pids());

        // If the root dies, the children are reparented by the system.
        // The processes are reparented to PID 0 here. It would be PID 1 on Linux.
        processes2[1].reparent(0);
        processes2[3].reparent(0);

        forest.refresh_from(shuffle(processes2).drain(..), &any_proc);
        assert_eq!(4, forest.size());
        assert_eq!(vec![proc1_pid, proc3_pid], sorted(forest.root_pids()));
        assert_eq!(0, forest.get_process(proc1_pid).unwrap().parent_pid());
        assert_eq!(0, forest.get_process(proc3_pid).unwrap().parent_pid());
    }

    #[test]
    /// Refresh a tree with a PID reused.
    ///
    /// A process dies and another process gets the same PID.
    fn test_refresh_pid_reused() {
        let mut factory = ProcessFactory::default();
        let mut processes1 = factory.with_parent_pids(&[(2, Some(0))], 3);
        let (first_proc_pid, first_proc_start) = {
            let proc = &mut processes1[1];
            proc.set_ttl(2);
            (proc.pid(), proc.stat().unwrap().starttime)
        };
        let mut processes2 = processes1.clone();
        let second_proc_start = {
            let proc = factory.builder().pid(first_proc_pid).parent_pid(0).build();
            assert_eq!(first_proc_pid, proc.pid());
            let start = proc.stat().unwrap().starttime;
            processes2[1] = proc;
            start
        };
        assert_ne!(first_proc_start, second_proc_start);

        let any_proc = AcceptAllProcesses::default();
        let mut forest = Forest::new();
        forest.refresh_from(shuffle(processes1).drain(..), &any_proc);
        let first_proc = forest.get_process(first_proc_pid).unwrap();
        assert_eq!(first_proc_pid, first_proc.pid());
        assert_eq!(first_proc_start, first_proc.start_time);

        forest.refresh_from(shuffle(processes2).drain(..), &any_proc);
        let second_proc = forest.get_process(first_proc_pid).unwrap();
        assert_eq!(first_proc_pid, second_proc.pid());
        assert_eq!(second_proc_start, second_proc.start_time);
    }
}
