// Oprs -- process monitor for Linux
// Copyright (C) 2024 Laurent Pelecq
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

use indextree::{Arena, NodeId};
use libc::pid_t;
use std::{
    collections::{BTreeMap, BTreeSet},
    iter::Iterator,
    path::PathBuf,
};

#[cfg(not(test))]
pub use procfs::{
    process::{all_processes, Process},
    ProcResult,
};

#[cfg(test)]
pub(crate) use crate::mocks::procfs::process::Process;

#[derive(thiserror::Error, Debug)]
pub enum ProcessError {
    #[error("unknown process {0}")]
    UnknownProcess(pid_t),
    #[error("cannot access processes")]
    CannotAccessProcesses,
}

pub type ProcessResult<T> = Result<T, ProcessError>;

/// Process name
///
/// Based of the first element of the command line if it exists or the name of
/// the executable.
fn process_name(process: &Process) -> Option<String> {
    process
        .cmdline()
        .map(|c| c.first().map(PathBuf::from))
        .ok()
        .flatten()
        .or_else(|| process.exe().ok())
        .map(|path| {
            path.file_name()
                .and_then(|os_name| os_name.to_str())
                .map(|s| s.to_string())
        })
        .flatten()
}

/// Process identifier: either the name or the PID into brackets.
pub fn process_identifier(process: &Process) -> String {
    process_name(process).unwrap_or_else(|| format!("[{}]", process.pid()))
}

#[derive(Debug)]
/// A slot for an existing or past process.
pub struct ProcessInstance {
    pid: pid_t,
    parent_pid: pid_t,
    start_time: u64,
    name: Option<String>,
    process: Process,
    hidden: bool,
}

impl ProcessInstance {
    fn new(process: Process) -> Result<Self, ProcessError> {
        let pid = process.pid();
        let stat = process
            .stat()
            .map_err(|_| ProcessError::UnknownProcess(pid))?;
        let name = process_name(&process);
        Ok(Self {
            pid: stat.pid,
            parent_pid: stat.ppid,
            start_time: stat.starttime,
            name,
            process,
            hidden: true,
        })
    }

    pub fn pid(&self) -> pid_t {
        self.pid
    }

    pub fn parent_pid(&self) -> pid_t {
        self.parent_pid
    }

    pub fn name<'a>(&'a self) -> Option<&'a str> {
        self.name.as_ref().map(|s| s.as_str())
    }

    pub fn process<'a>(&'a self) -> &'a Process {
        &self.process
    }

    pub fn hidden(&self) -> bool {
        self.hidden
    }

    pub fn hide(&mut self) {
        self.hidden = true;
    }

    pub fn show(&mut self) {
        self.hidden = false;
    }

    pub fn same_as(&self, other: &ProcessInstance) -> bool {
        self.pid == other.pid && self.start_time == other.start_time
    }
}

#[derive(Debug)]
pub struct RootIter<'a, 'b> {
    forest: &'a Forest,
    inner: std::collections::btree_set::Iter<'b, NodeId>,
}

impl<'a, 'b> Iterator for RootIter<'a, 'b> {
    type Item = &'a ProcessInstance;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|node_id| self.forest.get_known_info(*node_id))
    }
}

pub struct Descendants<'a, 'b> {
    forest: &'a Forest,
    inner: indextree::Descendants<'b, ProcessInstance>,
}

impl<'a, 'b> Iterator for Descendants<'a, 'b> {
    type Item = &'a ProcessInstance;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|node_id| self.forest.get_known_info(node_id))
    }
}

/// Forest of processes.
///
/// There may be multiple roots. All processes matching a predicate plus their ancestors
/// are in the forest.
#[derive(Debug)]
pub struct Forest {
    arena: Arena<ProcessInstance>,
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
    fn get_known_info<'a>(&'a self, node_id: NodeId) -> &'a ProcessInstance {
        self.arena
            .get(node_id)
            .expect("Internal error: dangling root in tree.")
            .get()
    }

    /// Attach a node in the tree.
    fn attach_node(&mut self, node_id: NodeId, pid: pid_t, parent_pid: pid_t) {
        self.processes.insert(pid, node_id);
        // It may be a parent of a root
        let adopted_ids = self
            .roots
            .iter()
            .filter(|root_id| self.get_known_info(**root_id).parent_pid() == pid)
            .copied()
            .collect::<Vec<NodeId>>();
        for root_id in adopted_ids {
            self.roots.remove(&root_id);
            node_id.append(root_id, &mut self.arena);
        }
        match self.processes.get(&parent_pid) {
            Some(parent_node_id) => {
                parent_node_id.append(node_id, &mut self.arena);
            }
            None => {
                self.roots.insert(node_id); // This node is a root
            }
        }
    }

    /// Add a process instance in the tree
    fn add_node(&mut self, info: ProcessInstance) {
        let pid = info.pid();
        let parent_pid = info.parent_pid();
        let node_id = self.arena.new_node(info);
        self.attach_node(node_id, pid, parent_pid);
    }

    /// Remove a node by PID.
    fn remove_node(&mut self, pid: pid_t, node_id: NodeId) {
        match node_id.children(&self.arena).next() {
            Some(_) => {
                if let Some(node) = self.arena.get_mut(node_id) {
                    node.get_mut().hide();
                }
            }
            None => {
                self.processes.remove(&pid);
                self.roots.remove(&node_id);
                node_id.remove(&mut self.arena);
            }
        }
    }

    /// Number of processes
    pub fn count(&self) -> usize {
        self.arena.count()
    }

    /// Get process with a given PID if it exists.
    pub fn get_process<'a>(&'a self, pid: pid_t) -> Option<&'a ProcessInstance> {
        self.processes
            .get(&pid)
            .map(|node_id| self.get_known_info(*node_id))
    }

    /// Remove process with a given PID. No error if it doesn't exists.
    pub fn remove_process(&mut self, pid: pid_t) {
        if let Some(node_id) = self.processes.get(&pid) {
            self.remove_node(pid, *node_id);
        }
    }

    /// Iterate roots
    pub fn iter_roots<'a: 'b, 'b>(&'a self) -> RootIter<'a, 'b> {
        RootIter {
            forest: &self,
            inner: self.roots.iter(),
        }
    }

    /// Descendants of a pid
    pub fn descendants(&self, pid: pid_t) -> ProcessResult<Descendants> {
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

    /// Refreshes the forest and return if it has changed.
    pub fn refresh_from<I, P>(&mut self, processes: I, predicate: P) -> bool
    where
        I: Iterator<Item = Process>,
        P: Fn(&ProcessInstance) -> bool,
    {
        let mut other_processes: BTreeMap<pid_t, ProcessInstance> = BTreeMap::new();
        let invalid_pids = self
            .arena
            .iter()
            .filter_map(|node| {
                let info = node.get();
                if info.process.is_alive() && predicate(info) {
                    Some(info.pid())
                } else {
                    None
                }
            })
            .collect::<Vec<pid_t>>();
        let mut changed = !invalid_pids.is_empty();
        for pid in invalid_pids {
            self.remove_process(pid);
        }
        for process in processes {
            let pid = process.pid();
            match ProcessInstance::new(process) {
                Ok(mut info) => {
                    if predicate(&info) {
                        info.show();
                        let mut parent_pid = info.parent_pid();
                        loop {
                            match other_processes.remove(&parent_pid) {
                                Some(parent_info) => {
                                    parent_pid = parent_info.parent_pid();
                                    self.add_node(parent_info);
                                    changed = true;
                                }
                                None => break,
                            }
                        }
                        match self.processes.get(&pid) {
                            Some(prev_node_id) => {
                                let prev_info = self.get_known_info(*prev_node_id);
                                if prev_info.same_as(&info) {
                                    if prev_info.parent_pid() != info.parent_pid() {
                                        // Same process but reparented.
                                        prev_node_id.detach(&mut self.arena);
                                        self.attach_node(*prev_node_id, pid, info.parent_pid());
                                        changed = true;
                                    }
                                } else {
                                    // Process ID has been reused. Remove the process.
                                    self.remove_node(prev_info.pid(), *prev_node_id);
                                    changed = true;
                                }
                            }
                            None => {
                                self.add_node(info);
                                changed = true;
                            }
                        }
                    } else {
                        other_processes.insert(pid, info);
                        changed = true;
                    }
                }
                Err(err) => {
                    log::info!("cannot stat process with id {}: {:?}", pid, err)
                }
            }
        }
        changed
    }

    #[cfg(not(test))]
    pub fn refresh<P>(&mut self, predicate: P) -> Result<bool, ProcessError>
    where
        P: Fn(&ProcessInstance) -> bool,
    {
        Ok(self.refresh_from(
            all_processes()
                .map_err(|_| ProcessError::CannotAccessProcesses)?
                .filter_map(ProcResult::ok),
            predicate,
        ))
    }
}

#[cfg(test)]
mod tests {

    use rand::seq::SliceRandom;
    use std::{collections::HashMap, time::Instant};

    use super::*;

    fn sorted<T: Clone, I>(input: I) -> Vec<T>
    where
        T: Clone + Ord,
        I: std::iter::IntoIterator<Item = T>,
    {
        let mut v = input.into_iter().collect::<Vec<T>>();
        v.sort();
        v
    }

    #[derive(Debug)]
    struct ProcessBuilder {
        pid: pid_t,
        parent_pid: pid_t,
        name: String,
        start_time: u64,
        ttl: Option<u16>,
    }

    impl ProcessBuilder {
        fn new(pid: pid_t, parent_pid: pid_t, name: &str, start_time: u64) -> Self {
            Self {
                pid,
                parent_pid,
                name: name.to_string(),
                start_time,
                ttl: None,
            }
        }

        fn pid(mut self, pid: pid_t) -> Self {
            self.pid = pid;
            self
        }

        fn parent_pid(mut self, parent_pid: pid_t) -> Self {
            self.parent_pid = parent_pid;
            self
        }

        fn name(mut self, name: &str) -> Self {
            self.name = name.to_string();
            self
        }

        fn start_time(mut self, start_time: u64) -> Self {
            self.start_time = start_time;
            self
        }

        fn ttl(mut self, ttl: u16) -> Self {
            self.ttl = Some(ttl);
            self
        }

        fn build(self) -> Process {
            let exe = format!("/bin/{}", self.name);
            Process::with_exe(self.pid, self.parent_pid, &exe, self.start_time, self.ttl)
        }
    }

    #[derive(Debug)]
    struct ProcessFactory {
        pid: pid_t,
        start_time: Instant,
        count: usize,
    }

    impl ProcessFactory {
        /// Return a builder with predefined name and pid and parent pid is the last pid.
        fn builder_with_pid(&mut self, pid: pid_t) -> ProcessBuilder {
            let name = format!("proc{}", self.count);
            let parent_pid = self.pid;
            if self.pid < pid {
                self.pid = pid;
            }
            self.count += 1;
            let start_time = self.start_time.elapsed().as_millis() as u64;
            ProcessBuilder::new(self.pid, parent_pid, &name, start_time)
        }

        /// Return a default builder with predefined name and parent pid is the last pid.
        fn builder(&mut self) -> ProcessBuilder {
            self.builder_with_pid(self.pid + 1)
        }

        fn last_pid(&self) -> pid_t {
            self.pid
        }

        /// Builds a forest based on constraits on parent pids.
        ///
        /// The vector `parents` gives the index of some parent of process.
        ///
        /// The first process has no parent. By default, process parent is the last process.
        ///
        /// Ex: [ (2, Some(0)), (3, None) ] means that the parent of process #2 is
        /// process #0 (the root) and that process #3 has no parent. It describes a
        /// forest of two trees.
        fn from_parent_pids(
            &mut self,
            constraints: &[(usize, Option<usize>)],
            count: usize,
        ) -> Vec<Process> {
            let constraints =
                HashMap::<usize, Option<usize>>::from_iter(constraints.iter().copied());
            let mut pids = Vec::new();
            (0..count)
                .map(|idx| {
                    let name = format!("proc{idx}");
                    let parent_pid = constraints
                        .get(&idx)
                        .map(|opt_idx| opt_idx.map(|idx| pids[idx]).unwrap_or(0))
                        .unwrap_or(self.last_pid());
                    let proc = self.builder().name(&name).parent_pid(parent_pid).build();
                    pids.push(proc.pid());
                    proc
                })
                .collect::<Vec<Process>>()
        }
    }

    impl Default for ProcessFactory {
        fn default() -> Self {
            Self {
                pid: 0,
                start_time: Instant::now(),
                count: 0,
            }
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
        let processes = factory.from_parent_pids(constraints, 8);
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
            .start_time(START_TIME)
            .name(NAME)
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
        forest.refresh_from(empty.drain(..), |info| info.pid() == 1);
    }

    #[test]
    /// Create a forest with one process.
    fn test_one_process() {
        const NAME: &str = "test";
        let mut factory = ProcessFactory::default();
        let mut forest = Forest::new();
        let mut processes = vec![factory.builder().name(NAME).build()];
        let first_pid = factory.last_pid();
        forest.refresh_from(processes.drain(..), |info| info.pid() == first_pid);
        let instance = forest.get_process(first_pid).unwrap();
        assert_eq!(first_pid, instance.pid());
        assert_eq!(NAME, instance.name().unwrap());
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
        let mut processes = factory.from_parent_pids(&[(3, Some(0)), (5, Some(2))], 6);

        let mut forest = Forest::new();
        forest.refresh_from(processes.drain(..), |_| true);
        let root_pids = forest.root_pids();
        assert_eq!(vec![1], root_pids);

        let expected_exe_tree = vec!["proc0", "proc1", "proc2", "proc5", "proc3", "proc4"];
        let exe_tree = forest
            .descendants(root_pids[0])
            .unwrap()
            .map(|p| p.name().unwrap_or("<unknown>").to_string())
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
        let mut processes = factory.from_parent_pids(&[(4, None)], 8);
        processes.shuffle(&mut rand::thread_rng());

        let mut forest = Forest::new();
        forest.refresh_from(processes.drain(..), |_| true);
        let root_pids = forest.root_pids();

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
        let mut processes = factory.from_parent_pids(&[(4, Some(2)), (5, Some(0))], 8);
        let proc3_pid = processes[3].pid();
        let proc4_pid = processes[4].pid();
        let proc6_pid = processes[6].pid();
        let proc7_pid = processes[7].pid();

        let mut forest = Forest::new();
        let predicate = |p: &ProcessInstance| p.pid() == proc4_pid || p.pid() == proc6_pid;
        forest.refresh_from(processes.drain(..), predicate);

        let root_pid = forest.root_pids()[0];

        assert_eq!(6, forest.count()); // Process 3 and 7 are discarded
        for pinfo in forest.descendants(root_pid).unwrap() {
            assert_eq!(predicate(pinfo), !pinfo.hidden());
            // Processes that have been discarded
            assert_ne!(proc3_pid, pinfo.pid());
            assert_ne!(proc7_pid, pinfo.pid());
        }
    }

    #[test]
    /// Refresh multiple times with different predicates.
    fn test_refresh_different_predicates() {
        panic!("test_refresh_different_predicates: unimplemented");
    }

    #[test]
    /// Refresh a tree with new processes.
    fn test_refresh_with_new_processes() {
        panic!("test_refresh_with_new_processes: unimplemented");
    }

    #[test]
    /// Refresh a tree with processes that die.
    fn test_refresh_with_old_processes() {
        panic!("test_refresh_with_old_processes: unimplemented");
    }

    #[test]
    /// Refresh a tree where the root process dies.
    ///
    /// - The tree must have multiple roots
    fn test_refresh_with_root_stopped() {
        panic!("test_refresh_with_root_stopped: unimplemented");
    }

    #[test]
    /// Refresh a tree with a process that is reparented (parent died).
    fn test_refresh_reparent() {
        panic!("test_refresh_reparent: unimplemented");
    }

    #[test]
    /// Refresh a tree with a PID reused.
    ///
    /// - A process dies and another process gets the same PID.
    fn test_refresh_pid_reused() {
        panic!("test_refresh_pid_reused: unimplemented");
    }
}
