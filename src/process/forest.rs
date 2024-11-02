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
pub(crate) use crate::mocks::procfs::{
    process::{all_processes, Process},
    ProcResult,
};

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
/// Information about for an existing or past process.
pub struct ProcessInfo {
    pid: pid_t,
    parent_pid: pid_t,
    start_time: u64,
    name: Option<String>,
    process: Process,
    hidden: bool,
}

impl ProcessInfo {
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

    pub fn same_as(&self, other: &ProcessInfo) -> bool {
        self.pid == other.pid && self.start_time == other.start_time
    }
}

#[derive(Debug)]
/// Iterator on a forest roots.
pub struct RootIter<'a, 'b> {
    forest: &'a Forest,
    inner: std::collections::btree_set::Iter<'b, NodeId>,
}

impl<'a, 'b> Iterator for RootIter<'a, 'b> {
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

impl<'a, 'b> Iterator for Descendants<'a, 'b> {
    type Item = &'a ProcessInfo;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|node_id| self.forest.get_known_info(node_id))
    }
}

#[derive(Debug)]
/// State used during refresh
struct RefreshState {
    processes: BTreeMap<pid_t, ProcessInfo>,
    old_nodes: BTreeSet<NodeId>,
    changed: bool,
}

impl RefreshState {
    fn new(arena: &Arena<ProcessInfo>) -> Self {
        Self {
            processes: BTreeMap::new(),
            old_nodes: BTreeSet::from_iter(
                arena.iter().map(|node| arena.get_node_id(node).unwrap()),
            ),
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
#[derive(Debug)]
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
    fn get_known_info<'a>(&'a self, node_id: NodeId) -> &'a ProcessInfo {
        self.arena
            .get(node_id)
            .expect("Internal error: dangling root in tree.")
            .get()
    }

    /// Attach a node in the tree.
    fn attach_node(
        &mut self,
        state: &mut RefreshState,
        node_id: NodeId,
        pid: pid_t,
        parent_pid: pid_t,
    ) {
        state.remove_old_node(&node_id);
        let is_new = self.processes.insert(pid, node_id).is_none();
        if is_new {
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
        }
        match self.processes.get(&parent_pid) {
            Some(parent_node_id) => {
                state.remove_old_node(parent_node_id);
                parent_node_id
                    .ancestors(&self.arena)
                    .for_each(|node_id| state.remove_old_node(&node_id));
                parent_node_id.append(node_id, &mut self.arena);
            }
            None => {
                self.roots.insert(node_id); // This node is a root
            }
        }
        state.changed = true;
    }

    /// Remove a node if it exists.
    fn remove_node(&mut self, state: &mut RefreshState, node_id: NodeId) {
        if let Some(node) = self.arena.get(node_id) {
            if !node.is_removed() {
                let pid = node.get().pid();
                self.processes.remove(&pid);
                node_id.remove(&mut self.arena);
            }
            self.roots.remove(&node_id);
            state.remove_old_node(&node_id);
        }
    }

    /// Add a process in the tree
    fn add_node(&mut self, state: &mut RefreshState, info: ProcessInfo) {
        let pid = info.pid();
        let parent_pid = info.parent_pid();

        match self.processes.get(&pid) {
            Some(prev_node_id) => {
                // A process with same PID exists. It can be a different process.
                let prev_info = self.get_known_info(*prev_node_id);
                if prev_info.same_as(&info) {
                    let new_parent_pid = info.parent_pid();
                    if prev_info.parent_pid() == new_parent_pid {
                        state.remove_old_node(&prev_node_id);
                    } else {
                        // Same process but reparented. Insert the new info where the
                        // previous was by making the new the parent of the previous one
                        // and removing the previous so the new inherits all the children.
                        log::debug!(
                            "process {} parent changed from {} to {}",
                            pid,
                            prev_info.parent_pid(),
                            new_parent_pid
                        );
                        let node_id = self.arena.new_node(info);
                        prev_node_id.detach(&mut self.arena);
                        node_id.append(*prev_node_id, &mut self.arena);
                        if self.roots.remove(&prev_node_id) {
                            self.roots.insert(node_id);
                        }
                        state.remove_old_node(prev_node_id);
                        prev_node_id.remove(&mut self.arena);
                        self.attach_node(state, node_id, pid, new_parent_pid);
                    }
                } else {
                    // Process ID has been reused. If the process had children,
                    // they have been reparented or will be. Remove it here to
                    // avoid the pid been removed.
                    self.remove_node(state, *prev_node_id);
                    let node_id = self.arena.new_node(info);
                    self.attach_node(state, node_id, pid, parent_pid);
                }
            }
            None => {
                let node_id = self.arena.new_node(info);
                self.attach_node(state, node_id, pid, parent_pid);
            }
        }
    }

    /// Remove a node and its children.
    fn remove_subtree(&mut self, state: &mut RefreshState, node_id: NodeId) {
        let child_node_ids = node_id.children(&self.arena).collect::<Vec<NodeId>>();
        for child_id in child_node_ids {
            self.remove_subtree(state, child_id);
        }
        self.remove_node(state, node_id);
    }

    /// Remove subtrees.
    fn remove_subtrees(&mut self, state: &mut RefreshState) {
        while let Some(node_id) = state.old_nodes.first() {
            self.remove_subtree(state, *node_id);
        }
    }

    /// Transfer a process and it's parents in the forest.
    ///
    /// It takes processes in the first list.
    fn transfer_ascendants(&mut self, state: &mut RefreshState, pid: pid_t) {
        let mut pid = pid;
        loop {
            // Add parent processes that have been found earlier but not
            // selected by the predicate to connect the tree.
            match state.processes.remove(&pid) {
                Some(info) => {
                    pid = info.parent_pid();
                    self.add_node(state, info);
                }
                None => break,
            }
        }
    }

    /// Number of processes
    pub fn size(&self) -> usize {
        self.processes.len()
    }

    /// Get process with a given PID if it exists.
    pub fn get_process<'a>(&'a self, pid: pid_t) -> Option<&'a ProcessInfo> {
        self.processes
            .get(&pid)
            .map(|node_id| self.get_known_info(*node_id))
    }

    /// Remove process with a given PID. No error if it doesn't exists.
    pub fn remove_process(&mut self, pid: pid_t) {
        if let Some(node_id) = self.processes.get(&pid).copied() {
            match node_id.children(&self.arena).next() {
                Some(_) => {
                    // If process has children, just hide it.
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
    }

    /// Iterate roots
    pub fn iter_roots<'a: 'b, 'b>(&'a self) -> RootIter<'a, 'b> {
        RootIter {
            forest: &self,
            inner: self.roots.iter(),
        }
    }

    /// Descendants of a pid
    ///
    /// Include the root process itself.
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
        P: Fn(&ProcessInfo) -> bool,
    {
        let mut state = RefreshState::new(&self.arena);
        for process in processes {
            let pid = process.pid();
            let is_alive = process.is_alive();
            match ProcessInfo::new(process) {
                Ok(mut info) => {
                    if is_alive && predicate(&info) {
                        info.show();
                        self.transfer_ascendants(&mut state, info.parent_pid());
                        self.add_node(&mut state, info);
                    } else {
                        state.processes.insert(pid, info);
                    }
                }
                Err(err) => {
                    log::info!("cannot stat process with id {pid}: {err:?}")
                }
            }
        }
        self.remove_subtrees(&mut state);
        state.changed
    }

    /// Refresh the forest with all the visible processes in the system if they match the predicate.
    pub fn refresh_if<P>(&mut self, predicate: P) -> Result<bool, ProcessError>
    where
        P: Fn(&ProcessInfo) -> bool,
    {
        Ok(self.refresh_from(
            all_processes()
                .map_err(|_| ProcessError::CannotAccessProcesses)?
                .filter_map(ProcResult::ok),
            predicate,
        ))
    }

    /// Refresh the forest with all the visible processes in the system.
    pub fn refresh(&mut self) -> Result<bool, ProcessError> {
        self.refresh_if(|_| true)
    }
}

#[cfg(test)]
mod tests {

    use rand::seq::SliceRandom;
    use std::collections::HashMap;

    use super::*;
    use crate::mocks::procfs::{reparent_process, ProcessBuilder};

    fn sorted<T: Clone, I>(input: I) -> Vec<T>
    where
        T: Clone + Ord,
        I: std::iter::IntoIterator<Item = T>,
    {
        let mut v = input.into_iter().collect::<Vec<T>>();
        v.sort();
        v
    }

    fn shuffle(mut processes: Vec<Process>) -> Vec<Process> {
        processes.shuffle(&mut rand::thread_rng());
        processes
    }

    #[derive(Debug)]
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

    impl Default for ProcessFactory {
        fn default() -> Self {
            Self { pid: 0, count: 0 }
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
        let pinfo = forest.get_process(first_pid).unwrap();
        assert_eq!(first_pid, pinfo.pid());
        assert_eq!(first_pid, pinfo.process().pid());
        assert_eq!(NAME, pinfo.name().unwrap());
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
        let mut processes = shuffle(factory.from_parent_pids(&[(4, None)], 8));

        let mut forest = Forest::new();
        forest.refresh_from(processes.drain(..), |_| true);

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
        let predicate = |p: &ProcessInfo| p.pid() == proc4_pid || p.pid() == proc6_pid;
        forest.refresh_from(processes.drain(..), predicate);

        let root_pid = forest.root_pids()[0];

        assert_eq!(6, forest.size()); // Process 3 and 7 are discarded
        for pinfo in forest.descendants(root_pid).unwrap() {
            assert_eq!(predicate(pinfo), !pinfo.hidden());
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
        let mut factory = ProcessFactory::default();
        let processes1 = factory.from_parent_pids(&[(4, Some(2)), (5, Some(0))], 8);
        let processes2 = processes1.clone();
        let proc3_pid = processes1[3].pid();
        let proc4_pid = processes1[4].pid();

        let mut forest = Forest::new();
        forest.refresh_from(shuffle(processes1).drain(..), |p| p.pid() == proc3_pid);
        assert!(forest.get_process(proc3_pid).is_some());
        assert!(forest.get_process(proc4_pid).is_none());

        forest.refresh_from(shuffle(processes2).drain(..), |p| p.pid() == proc4_pid);
        assert!(forest.get_process(proc3_pid).is_none());
        assert!(forest.get_process(proc4_pid).is_some());
    }

    #[test]
    /// Refresh a tree with new processes.
    fn test_refresh_with_new_processes() {
        let mut factory = ProcessFactory::default();
        let root = factory.build();
        let root_pid = root.pid();
        let mut processes = Vec::new();
        processes.push(root);

        let mut forest = Forest::new();
        forest.refresh_from(processes.clone().drain(..), |_| true);
        for count in 2..6 {
            // Add a new process at each loop.
            let proc = factory.builder().parent_pid(root_pid).build();
            processes.push(proc);
            forest.refresh_from(shuffle(processes.clone()).drain(..), |_| true);
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
        let mut processes1 = factory.from_parent_pids(&[(3, Some(0))], 5);
        let proc2_pid = processes1[2].pid();
        let mut processes2 = processes1.clone();

        let mut forest = Forest::new();
        forest.refresh_from(processes1.drain(..), |_| true);
        assert_eq!(5, forest.size());

        let mut ttl = 3;
        let proc = factory.builder().parent_pid(proc2_pid).ttl(ttl).build();
        let proc_pid = proc.pid();
        assert_eq!(ttl, proc.ttl().unwrap());
        processes2.push(proc);

        loop {
            ttl = ttl.checked_sub(1).unwrap_or(0);
            forest.refresh_from(processes2.clone().drain(..), |_| true);
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
        forest.refresh_from(processes3.drain(..), |_| true);
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
        let processes1 = factory.from_parent_pids(&[(3, Some(0))], 5);
        let mut root = processes1[0].clone();
        let root_pid = root.pid();
        root.set_ttl(1);
        // If the root dies, the children are reparented by the system.
        // The processes are reparented to PID 0 here. It would be PID 1 on Linux.
        let processes2 = vec![
            root,
            reparent_process(&processes1[1], 0),
            processes1[2].clone(),
            reparent_process(&processes1[3], 0),
            processes1[4].clone(),
        ];
        let proc1_pid = processes2[1].pid();
        let proc3_pid = processes2[3].pid();

        let mut forest = Forest::new();
        forest.refresh_from(shuffle(processes1).drain(..), |_| true);
        assert_eq!(5, forest.size());
        assert_eq!(vec![root_pid], forest.root_pids());

        forest.refresh_from(shuffle(processes2).drain(..), |_| true);
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
        let processes1 = factory.from_parent_pids(&[(2, Some(0))], 3);
        let (first_proc_pid, first_proc_start) = {
            let proc = &processes1[1];
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

        let mut forest = Forest::new();
        forest.refresh_from(shuffle(processes1).drain(..), |_| true);
        let first_proc = forest.get_process(first_proc_pid).unwrap();
        assert_eq!(first_proc_pid, first_proc.pid());
        assert_eq!(first_proc_start, first_proc.start_time);

        forest.refresh_from(shuffle(processes2).drain(..), |_| true);
        let second_proc = forest.get_process(first_proc_pid).unwrap();
        assert_eq!(first_proc_pid, second_proc.pid());
        assert_eq!(second_proc_start, second_proc.start_time);
    }
}
