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

use getset::{Getters, Setters};
use libc::pid_t;
use smart_default::SmartDefault;
use std::{collections::BTreeSet, fmt};
use strum::Display as StrumDisplay;

use crate::{
    console::{Event, Key},
    process::ProcessIdentity,
};

use super::types::BoundedFifo;

/// Standard keys
const KEY_ENTER: Key = Key::Char('\n');
const KEY_ESCAPE: Key = Key::Esc;
const KEY_FASTER: Key = Key::Char(KEY_FASTER_CHAR);
const KEY_FASTER_CHAR: char = '+';
const KEY_FILTERS: Key = Key::Char('f');
const KEY_FILTER_ACTIVE: Key = Key::Char('a');
const KEY_FILTER_NONE: Key = Key::Char('n');
const KEY_FILTER_USER: Key = Key::Char('u');
const KEY_GOTO_TBL_BOTTOM: Key = Key::CtrlEnd;
const KEY_GOTO_TBL_LEFT: Key = Key::Home;
const KEY_GOTO_TBL_RIGHT: Key = Key::End;
const KEY_GOTO_TBL_TOP: Key = Key::CtrlHome;
const KEY_HELP: Key = Key::Char('?');
const KEY_LIMITS: Key = Key::Char('l');
const KEY_MARK_CLEAR: Key = Key::Ctrl('c');
const KEY_MARK_TOGGLE: Key = Key::Char(' ');
const KEY_QUIT: Key = Key::Char('q');
const KEY_SCOPE: Key = Key::Char('s');
const KEY_SEARCH: Key = Key::Char('/');
const KEY_SEARCH_CANCEL: Key = Key::Ctrl('c');
const KEY_SELECT_NEXT: Key = Key::Char(KEY_SELECT_NEXT_CHAR);
const KEY_SELECT_NEXT_CHAR: char = 'n';
const KEY_SELECT_PARENT: Key = Key::Char('p');
const KEY_SELECT_PREVIOUS: Key = Key::Char(KEY_SELECT_PREVIOUS_CHAR);
const KEY_SELECT_PREVIOUS_CHAR: char = 'N';
const KEY_SELECT_ROOT_PID: Key = Key::Char('r');
const KEY_UNSELECT_ROOT_PID: Key = Key::Char('R');
const KEY_SLOWER: Key = Key::Char(KEY_SLOWER_CHAR);
const KEY_SLOWER_CHAR: char = '-';

macro_rules! try_return {
    ($option:expr) => {
        match $option {
            Some(value) => return value,
            None => (),
        }
    };
}

/// User action
#[derive(Clone, Debug)]
pub enum Action {
    None,
    ChangeScope,
    DivideTimeout(u16),
    Filters,
    FilterNone,
    FilterUser,
    FilterActive,
    GotoTableBottom,
    GotoTableLeft,
    GotoTableRight,
    GotoTableTop,
    SwitchToHelp,
    SwitchBack,
    SwitchToDetails,
    SwitchToLimits,
    ClearMarks,
    ToggleMarks,
    MultiplyTimeout(u16),
    Quit,
    ScrollLeft,
    ScrollLineDown,
    ScrollLineUp,
    ScrollPageDown,
    ScrollPageUp,
    ScrollRight,
    SearchCancel,
    SearchEnter,
    SearchExit,
    SearchPop,
    SelectNext,
    SelectPrevious,
    SelectParent,
    SelectRootPid,
    UnselectRootPid,
    SearchPush(char),
}

/// Keymap
#[derive(Clone, Copy, Debug, StrumDisplay, PartialEq)]
pub enum KeyMap {
    #[strum(serialize = "main")]
    Main,
    #[strum(serialize = "help")]
    Help,
    #[strum(serialize = "filters")]
    Filters,
    #[strum(serialize = "incremental search")]
    IncrementalSearch,
    #[strum(serialize = "details")]
    Details,
    #[strum(serialize = "process")]
    Process,
}

impl KeyMap {
    /// Convert an input event to an action
    pub fn action_from_event(self, evt: Event) -> Action {
        //log::debug!("event: {evt:?}");
        match self {
            KeyMap::IncrementalSearch => match evt {
                Event::Key(KEY_ENTER) => Action::SearchExit,
                Event::Key(Key::Char(c)) => Action::SearchPush(c),
                Event::Key(Key::Backspace) => Action::SearchPop,
                Event::Key(KEY_SEARCH_CANCEL) => Action::SearchCancel,
                _ => Action::None,
            },
            KeyMap::Help | KeyMap::Process => match evt {
                Event::Key(KEY_QUIT) | Event::Key(KEY_ESCAPE) => Action::SwitchBack,
                Event::Key(Key::PageDown) => Action::ScrollPageDown,
                Event::Key(Key::PageUp) => Action::ScrollPageUp,
                _ => Action::None,
            },
            KeyMap::Details => match evt {
                Event::Key(KEY_QUIT) | Event::Key(KEY_ESCAPE) => Action::SwitchBack,
                Event::Key(KEY_SELECT_PARENT) => Action::SelectParent,
                Event::Key(KEY_LIMITS) => Action::SwitchToLimits,
                Event::Key(Key::PageDown) => Action::ScrollPageDown,
                Event::Key(Key::PageUp) => Action::ScrollPageUp,
                _ => Action::None,
            },
            KeyMap::Filters => match evt {
                Event::Key(KEY_FILTER_NONE) => Action::FilterNone,
                Event::Key(KEY_FILTER_USER) => Action::FilterUser,
                Event::Key(KEY_FILTER_ACTIVE) => Action::FilterActive,
                _ => Action::None,
            },
            KeyMap::Main => match evt {
                Event::Key(KEY_FASTER) => Action::DivideTimeout(2),
                Event::Key(KEY_GOTO_TBL_BOTTOM) => Action::GotoTableBottom,
                Event::Key(KEY_GOTO_TBL_LEFT) => Action::GotoTableLeft,
                Event::Key(KEY_GOTO_TBL_RIGHT) => Action::GotoTableRight,
                Event::Key(KEY_GOTO_TBL_TOP) => Action::GotoTableTop,
                Event::Key(KEY_ENTER) => Action::SwitchToDetails,
                Event::Key(KEY_HELP) => Action::SwitchToHelp,
                Event::Key(KEY_MARK_CLEAR) => Action::ClearMarks,
                Event::Key(KEY_MARK_TOGGLE) => Action::ToggleMarks,
                Event::Key(KEY_FILTERS) => Action::Filters,
                Event::Key(KEY_SCOPE) => Action::ChangeScope,
                Event::Key(KEY_SEARCH) => Action::SearchEnter,
                Event::Key(KEY_SELECT_PREVIOUS) => Action::SelectPrevious,
                Event::Key(KEY_SELECT_NEXT) => Action::SelectNext,
                Event::Key(KEY_SELECT_ROOT_PID) => Action::SelectRootPid,
                Event::Key(KEY_UNSELECT_ROOT_PID) => Action::UnselectRootPid,
                Event::Key(KEY_SLOWER) => Action::MultiplyTimeout(2),
                Event::Key(KEY_QUIT) | Event::Key(KEY_ESCAPE) => Action::Quit,
                Event::Key(Key::PageDown) => Action::ScrollPageDown,
                Event::Key(Key::PageUp) => Action::ScrollPageUp,
                Event::Key(Key::Down) => Action::ScrollLineDown,
                Event::Key(Key::Up) => Action::ScrollLineUp,
                Event::Key(Key::Left) => Action::ScrollLeft,
                Event::Key(Key::Right) => Action::ScrollRight,
                _ => Action::None,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum KeyMapSet {
    OnlyIn(KeyMap),
    ExceptIn(KeyMap),
}

impl KeyMapSet {
    pub fn contains(&self, keymap: KeyMap) -> bool {
        match self {
            Self::OnlyIn(valid) if keymap == *valid => true,
            Self::ExceptIn(invalid) if keymap != *invalid => true,
            _ => false,
        }
    }
}

/// Menu entry with a key and a label.
#[derive(Debug, Getters)]
pub struct MenuEntry {
    key: String,
    #[getset(get = "pub")]
    label: &'static str,
    #[getset(get = "pub")]
    keymaps: KeyMapSet,
}

impl MenuEntry {
    pub fn new(key: String, label: &'static str, keymaps: KeyMapSet) -> Self {
        Self {
            key,
            label,
            keymaps,
        }
    }

    pub fn with_key(key: Key, label: &'static str, keymaps: KeyMapSet) -> Self {
        Self::new(MenuEntry::key_name(key), label, keymaps)
    }

    pub fn key(&self) -> &str {
        self.key.as_str()
    }

    fn key_name(key: Key) -> String {
        match key {
            Key::Backspace => "⌫".to_string(),
            Key::Left => "←".to_string(),
            Key::Right => "→".to_string(),
            Key::Up => "↑".to_string(),
            Key::Down => "↓".to_string(),
            Key::PageUp => "⇞".to_string(),
            Key::PageDown => "⇟".to_string(),
            Key::Home => "⇱".to_string(),
            Key::CtrlHome => "^⇱".to_string(),
            Key::End => "⇲".to_string(),
            Key::CtrlEnd => "^⇲".to_string(),
            Key::BackTab => "⇤".to_string(),
            Key::Delete => "⌧".to_string(),
            Key::Insert => "Ins".to_string(),
            Key::F(num) => format!("F{num}"),
            Key::Char('\t') => "⇥".to_string(),
            Key::Char(' ') => "Spc".to_string(),
            Key::Char(ch) => format!("{ch}"),
            Key::Alt(ch) => format!("M-{ch}"),
            Key::Ctrl(ch) => format!("C-{ch}"),
            Key::Null => "\\0".to_string(),
            _ => "?".to_string(),
        }
    }
}

/// Return the menu
pub fn menu() -> Vec<MenuEntry> {
    vec![
        MenuEntry::with_key(KEY_QUIT, "Quit", KeyMapSet::ExceptIn(KeyMap::Filters)),
        MenuEntry::with_key(KEY_HELP, "Help", KeyMapSet::OnlyIn(KeyMap::Main)),
        MenuEntry::new(
            format!("{KEY_SELECT_NEXT_CHAR}/{KEY_SELECT_PREVIOUS_CHAR}",),
            "Next/Prev",
            KeyMapSet::OnlyIn(KeyMap::Main),
        ),
        MenuEntry::with_key(KEY_SEARCH, "Search", KeyMapSet::OnlyIn(KeyMap::Main)),
        MenuEntry::with_key(KEY_LIMITS, "Limits", KeyMapSet::OnlyIn(KeyMap::Details)),
        MenuEntry::with_key(KEY_FILTERS, "Filters", KeyMapSet::OnlyIn(KeyMap::Main)),
        MenuEntry::with_key(
            KEY_SELECT_PARENT,
            "Parent",
            KeyMapSet::OnlyIn(KeyMap::Details),
        ),
        MenuEntry::with_key(KEY_SELECT_ROOT_PID, "Root", KeyMapSet::OnlyIn(KeyMap::Main)),
        MenuEntry::new(
            format!("{KEY_FASTER_CHAR}/{KEY_SLOWER_CHAR}"),
            "Speed",
            KeyMapSet::OnlyIn(KeyMap::Main),
        ),
        MenuEntry::with_key(KEY_FILTER_NONE, "None", KeyMapSet::OnlyIn(KeyMap::Filters)),
        MenuEntry::with_key(KEY_FILTER_USER, "User", KeyMapSet::OnlyIn(KeyMap::Filters)),
        MenuEntry::with_key(
            KEY_FILTER_ACTIVE,
            "Active",
            KeyMapSet::OnlyIn(KeyMap::Filters),
        ),
    ]
}

/// Search bar state
#[derive(Debug, SmartDefault)]
pub enum SearchState {
    #[default]
    Incremental(Vec<char>),
    Fixed(String),
}

impl fmt::Display for SearchState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let s = match self {
            Self::Incremental(chars) => &chars.iter().cloned().collect::<String>(),
            Self::Fixed(s) => s,
        };
        write!(f, "{}", s)
    }
}

/// Search action
#[derive(Clone, Copy, Debug, SmartDefault)]
pub enum BookmarkAction {
    #[default]
    None,
    /// Select first line
    FirstLine,
    /// Select last line
    LastLine,
    /// Select previous line
    PreviousLine,
    /// Select next line
    NextLine,
    /// Select previous page
    PreviousPage,
    /// Select next page
    NextPage,
    /// Select previous search occurrence or mark.
    Previous,
    /// Select next search occurrence or mark.
    Next,
    /// Current selected line if it still matched, else the next matching.
    ClosestMatch,
    /// Invert the marks of the matched lines or the current selection.
    ToggleMarks,
}

/// Action to edit search bar
#[derive(Debug)]
pub enum SearchEdit {
    Push(char),
    Pop,
}

/// Search bar
#[derive(Debug, Default, Getters, Setters)]
pub struct SearchBar {
    #[getset(get = "pub")]
    state: SearchState,
}

impl SearchBar {
    pub fn pattern(&self) -> String {
        self.state.to_string()
    }

    /// Turn the fixed string search string into an incremental search.
    pub fn thaw(&mut self) {
        if let SearchState::Fixed(ref pattern) = self.state {
            self.state = SearchState::Incremental(pattern.chars().collect::<Vec<char>>());
        }
    }

    /// Turn the incremental search string into a fixed string search.
    pub fn freeze(&mut self) -> bool {
        !match &self.state {
            SearchState::Incremental(_) => {
                let pattern = self.state.to_string();
                self.state = SearchState::Fixed(pattern.to_string());
                pattern.is_empty()
            }
            SearchState::Fixed(pattern) => pattern.is_empty(),
        }
    }

    /// Push a char on the incremental search string.
    pub fn push(&mut self, c: char) {
        match self.state {
            SearchState::Incremental(ref mut chars) => chars.push(c),
            _ => panic!("cannot push a character to the search string"),
        }
    }

    /// Pop a char from the incremental search string.
    pub fn pop(&mut self) {
        match self.state {
            SearchState::Incremental(ref mut chars) => {
                let _ = chars.pop();
            }
            _ => panic!("cannot pop a character from the search string"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LinePid {
    pub lineno: usize,
    pub pid: pid_t,
}

impl LinePid {
    fn new(lineno: usize, pid: pid_t) -> Self {
        Self { lineno, pid }
    }

    fn distance(a: usize, b: usize) -> usize {
        if a < b {
            b - a
        } else {
            a - b
        }
    }

    fn pid_index_in(&self, v: &[LinePid]) -> Option<usize> {
        v.iter().enumerate().find_map(|(index, lp)| {
            if lp.pid == self.pid {
                Some(index)
            } else {
                None
            }
        })
    }

    /// Return the item before this PID in the list or before this line if PID is not found.
    fn previous_in<'a>(&self, v: &'a [LinePid]) -> Option<&'a LinePid> {
        let len = v.len();
        match self.pid_index_in(v) {
            Some(index) => v.get((index + len - 1) % len),
            None if len > 0 => v
                .iter()
                .rev()
                .find(|lp| lp.lineno <= self.lineno)
                .or(v.last()),
            None => None,
        }
    }

    /// Return the item after this PID in the list or after this line if PID is not found.
    fn next_in<'a>(&self, v: &'a [LinePid]) -> Option<&'a LinePid> {
        let len = v.len();
        match self.pid_index_in(v) {
            Some(index) => v.get((index + len + 1) % len),
            None if len > 0 => v.iter().find(|lp| lp.lineno >= self.lineno).or(v.first()),
            None => None,
        }
    }

    /// Return the item with this PID or the closest from this line.
    fn closest_in<'a>(&self, v: &'a [LinePid]) -> Option<&'a LinePid> {
        v.iter().find(|lp| lp.pid == self.pid).or_else(|| {
            let mut distance = 0;
            let mut candidate = None;
            for lp in v {
                match candidate {
                    Some(_) => {
                        let new_distance = LinePid::distance(lp.lineno, self.lineno);
                        if new_distance < distance {
                            distance = new_distance;
                            candidate = Some(lp);
                        }
                    }
                    None => {
                        distance = LinePid::distance(lp.lineno, self.lineno);
                        candidate = Some(lp);
                    }
                }
            }
            candidate
        })
    }
}

/// Search bar
#[derive(Debug, Default, Getters, Setters)]
pub struct Bookmarks {
    /// PID at the line under the cursor.
    #[getset(get = "pub")]
    selected: Option<LinePid>,
    /// Optional search pattern.
    #[getset(get = "pub")]
    search: Option<SearchBar>,
    /// PIDs marked in the selection.
    #[getset(get = "pub")]
    marks: BTreeSet<pid_t>,
    /// Action for next round.
    #[getset(get = "pub", set = "pub")]
    action: BookmarkAction,
}

impl Bookmarks {
    /// Recenter vertically on a given position.
    ///
    /// # Arguments
    ///
    /// * `center` - The position to recenter to.
    /// * `top` - The first visible line.
    /// * `height` - The height of the visible area.
    /// * `force` - Recenter even if the position is already visible.
    fn recenter(center: usize, top: usize, height: usize, force: bool) -> usize {
        let bottom = top + height;
        if force || center < top || center >= bottom {
            center.saturating_sub(std::cmp::max(1, height / 2))
        } else {
            top
        }
    }

    /// Check if PID is selected.
    pub fn is_selected(&self, pid: pid_t) -> bool {
        self.selected.map(|s| s.pid == pid).unwrap_or(false)
    }

    /// Check if PID is marked.
    pub fn is_marked(&self, pid: pid_t) -> bool {
        self.marks.contains(&pid)
    }

    /// Clear marks
    pub fn clear_marks(&mut self) {
        self.marks.clear();
    }

    /// Start an incremental search.
    pub fn incremental_search(&mut self) {
        match self.search {
            Some(ref mut search) => search.thaw(),
            None => self.search = Some(SearchBar::default()),
        }
    }

    /// Switch to a fixed string search.
    pub fn fixed_search(&mut self) {
        if let Some(ref mut search) = self.search {
            if !search.freeze() {
                self.clear_search();
            }
        }
    }

    /// Clear search
    pub fn clear_search(&mut self) {
        self.search = None;
    }

    /// Return the search pattern if any.
    pub fn search_pattern(&self) -> Option<String> {
        self.search.as_ref().map(|s| s.pattern())
    }

    /// Whether there is an ongoing incremental search.
    pub fn is_incremental_search(&self) -> bool {
        match self.search {
            Some(ref search) => matches!(search.state, SearchState::Incremental(_)),
            None => false,
        }
    }

    /// Edit search pattern
    pub fn edit_search(&mut self, edit: SearchEdit) {
        if let Some(ref mut search) = self.search {
            match edit {
                SearchEdit::Push(c) => search.push(c),
                SearchEdit::Pop => search.pop(),
            }
        }
    }

    /// Set the selection and recenter it.
    fn select(
        &mut self,
        lineno: usize,
        pid: pid_t,
        top: usize,
        height: usize,
        force: bool,
    ) -> usize {
        self.selected = Some(LinePid::new(lineno, pid));
        Bookmarks::recenter(lineno, top, height, force)
    }

    /// Apply a function on the selection.
    fn change_selection_in_ring<F>(&mut self, ring: &[LinePid], f: F) -> usize
    where
        F: Fn(&LinePid, &[LinePid]) -> Option<LinePid>,
    {
        self.selected = match self.selected.as_ref() {
            Some(lp) => f(lp, ring).or(Some(*lp)),
            None => ring.first().copied(),
        };
        self.selected.map(|lp| lp.lineno).unwrap_or(0)
    }

    /// Select the previous PID if the current PID is the current selection.
    ///
    /// The offset is based on the `previous_pids` FIFO size. It's one
    /// for previous line or the page size.
    fn select_previous(
        &mut self,
        previous_pids: &BoundedFifo<pid_t>,
        current_lineno: usize,
        current_pid: pid_t,
        top: usize,
        height: usize,
    ) -> Option<usize> {
        let force = previous_pids.capacity() > 1; // Moving by pages.
        match self.selected {
            Some(selected) if current_pid == selected.pid => {
                // If current PID is the selected PID, the front of previous
                // PIDs is the one to select. If there is no previous PID,
                // just stay on the selection.
                let lineno = match previous_pids.front() {
                    Some(prev_pid) => {
                        let lineno = current_lineno - previous_pids.len();
                        self.selected = Some(LinePid::new(lineno, *prev_pid));
                        lineno
                    }
                    None => selected.lineno,
                };
                Some(Bookmarks::recenter(lineno, top, height, force))
            }
            Some(_) => None, // Current is not the one we are looking for.
            None => {
                // No selection, select this one (should be the first line).
                Some(self.select(current_lineno, current_pid, top, height, force))
            }
        }
    }

    /// Select the current PID is the next after the current selection.
    ///
    /// The offset is based on the `previous_pids` FIFO size. It's one
    /// for next line or the page size.
    fn select_next(
        &mut self,
        previous_pids: &BoundedFifo<pid_t>,
        current_lineno: usize,
        current_pid: pid_t,
        top: usize,
        height: usize,
    ) -> Option<usize> {
        let force = previous_pids.len() > 1; // Moving by pages.
        match (self.selected.map(|s| s.pid), previous_pids.front()) {
            (Some(selected_pid), Some(prev_pid)) if *prev_pid == selected_pid => {
                Some(self.select(current_lineno, current_pid, top, height, force))
            }
            (None, _) => Some(self.select(current_lineno, current_pid, top, height, force)),
            _ => None,
        }
    }

    /// Toggle the mark for the given PID.
    fn toggle_mark(&mut self, pid: pid_t) {
        if !self.marks.remove(&pid) {
            self.marks.insert(pid);
        }
    }

    /// Execute the action and return the vertical offset.
    ///
    /// * `occurrences` - The set of matching pid in case of search.
    /// * `lines` - The lines of process identities.
    /// * `top` - The first visible line (current vertical offset).
    /// * `height` - The height of the visible area.
    pub fn execute<I, P>(
        &mut self,
        occurrences: &mut BTreeSet<pid_t>,
        lines: I,
        top: usize,
        height: usize,
    ) -> usize
    where
        I: Iterator<Item = P>,
        P: ProcessIdentity,
    {
        let action = self.action;
        self.action = match self.search {
            Some(_) => BookmarkAction::ClosestMatch,
            None => BookmarkAction::None,
        };
        occurrences.clear();
        let page_size = match action {
            BookmarkAction::PreviousPage | BookmarkAction::NextPage => std::cmp::max(1, height / 2),
            _ => 1,
        };
        let mut pid_at_line = None;
        let mut last_lineno = None;
        let mut previous_pids = BoundedFifo::new(page_size);
        let mut matches = Vec::new();
        let mut marks = Vec::new();
        let pattern = self.search_pattern();

        for (lineno, pi) in lines.enumerate() {
            let pid = pi.pid();
            if pid == 0 {
                continue;
            }
            if self.marks.contains(&pid) {
                marks.push(LinePid::new(lineno, pid));
            }
            if let Some(ref mut selected) = self.selected {
                if selected.pid == pid {
                    selected.lineno = lineno;
                }
                if selected.lineno == lineno {
                    pid_at_line = Some(pid);
                }
            }
            match action {
                BookmarkAction::None => match self.selected {
                    Some(ref mut selected) if pid == selected.pid => {
                        selected.lineno = lineno;
                        return Bookmarks::recenter(lineno, top, height, false);
                    }
                    Some(_) => (),
                    None => return Bookmarks::recenter(0, top, height, false),
                },
                BookmarkAction::FirstLine => return self.select(lineno, pid, top, height, true),
                BookmarkAction::LastLine => last_lineno = Some(lineno),
                BookmarkAction::PreviousLine | BookmarkAction::PreviousPage => {
                    try_return!(self.select_previous(&previous_pids, lineno, pid, top, height))
                }
                BookmarkAction::NextLine | BookmarkAction::NextPage => {
                    try_return!(self.select_next(&previous_pids, lineno, pid, top, height))
                }
                BookmarkAction::Previous
                | BookmarkAction::Next
                | BookmarkAction::ClosestMatch
                | BookmarkAction::ToggleMarks => {
                    if self.search_pattern().is_none()
                        && self.marks.is_empty()
                        && !matches!(action, BookmarkAction::ToggleMarks)
                    {
                        return self.select(lineno, pid, top, height, true);
                    }
                    if let Some(pattern) = pattern.as_ref() {
                        if pi.name().contains(pattern) {
                            matches.push(LinePid::new(lineno, pid));
                            occurrences.insert(pid);
                        }
                    }
                }
            }
            previous_pids.push(pid);
        }

        self.marks = BTreeSet::from_iter(marks.iter().map(|lp| lp.pid)); // Keep only marks on existing PIDs.
        let match_count = matches.len();
        let ring = match pattern {
            Some(_) => &matches,
            None => &marks,
        };
        let new_top = match action {
            BookmarkAction::None => top,
            BookmarkAction::FirstLine => 0,
            BookmarkAction::LastLine => {
                let lineno = last_lineno.expect("internal error: last line must be set");
                self.selected = previous_pids.back().map(|pid| LinePid::new(lineno, *pid));
                lineno
            }
            BookmarkAction::PreviousLine
            | BookmarkAction::PreviousPage
            | BookmarkAction::NextLine
            | BookmarkAction::NextPage => match (self.selected, pid_at_line) {
                (Some(selected), Some(pid)) => {
                    let lineno = selected.lineno;
                    self.selected = Some(LinePid::new(lineno, pid));
                    lineno
                }
                _ => {
                    self.selected = None;
                    0
                }
            },
            BookmarkAction::Previous => {
                self.change_selection_in_ring(ring, |s, ring| s.previous_in(ring).copied())
            }
            BookmarkAction::Next => {
                self.change_selection_in_ring(ring, |s, ring| s.next_in(ring).copied())
            }
            BookmarkAction::ClosestMatch => {
                self.change_selection_in_ring(&matches, |s, ring| s.closest_in(ring).copied())
            }
            BookmarkAction::ToggleMarks => {
                if occurrences.is_empty() {
                    if let Some(selected) = self.selected {
                        self.toggle_mark(selected.pid);
                    }
                } else {
                    occurrences.iter().for_each(|pid| self.toggle_mark(*pid));
                    self.clear_search();
                    occurrences.clear();
                }
                self.selected.map(|s| s.lineno).unwrap_or(0)
            }
        };
        Bookmarks::recenter(new_top, top, height, match_count > 0)
    }
}
