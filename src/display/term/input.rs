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

use super::types::Scroll;

/// Standard keys
const KEY_ENTER: Key = Key::Char('\n');
const KEY_ENV: Key = Key::Char('e');
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
const KEY_PAGE_LEFT: Key = Key::BackTab;
const KEY_PAGE_RIGHT: Key = Key::Char('\t');
const KEY_QUIT: Key = Key::Char('q');
const KEY_SCOPE: Key = Key::Char('s');
const KEY_SEARCH: Key = Key::Char('/');
const KEY_SEARCH_CANCEL: Key = Key::Ctrl('c');
const KEY_SEARCH_NEXT: Key = Key::Ctrl('n');
const KEY_SEARCH_PREVIOUS: Key = Key::Ctrl('N');
const KEY_SELECT_NEXT: Key = Key::Char(KEY_SELECT_NEXT_CHAR);
const KEY_SELECT_NEXT_CHAR: char = 'n';
const KEY_SELECT_PARENT: Key = Key::Char('p');
const KEY_SELECT_PREVIOUS: Key = Key::Char(KEY_SELECT_PREVIOUS_CHAR);
const KEY_SELECT_PREVIOUS_CHAR: char = 'N';
const KEY_SELECT_ROOT_PID: Key = Key::Char('r');
const KEY_UNSELECT_ROOT_PID: Key = Key::Char('R');
const KEY_SLOWER: Key = Key::Char(KEY_SLOWER_CHAR);
const KEY_SLOWER_CHAR: char = '-';

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
    SwitchToEnvironment,
    ClearMarks,
    ToggleMarks,
    MultiplyTimeout(u16),
    Quit,
    ScrollLeft,
    ScrollLineDown,
    ScrollLineUp,
    ScrollPageDown,
    ScrollPageLeft,
    ScrollPageRight,
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
        match self {
            KeyMap::IncrementalSearch => match evt {
                Event::Key(KEY_ENTER) => Action::SearchExit,
                Event::Key(Key::Char(c)) => Action::SearchPush(c),
                Event::Key(Key::Backspace) => Action::SearchPop,
                Event::Key(KEY_SEARCH_PREVIOUS) => Action::SelectPrevious,
                Event::Key(KEY_SEARCH_NEXT) => Action::SelectNext,
                Event::Key(KEY_SEARCH_CANCEL) => Action::SearchCancel,
                _ => Action::None,
            },
            KeyMap::Help => match evt {
                Event::Key(KEY_QUIT) | Event::Key(KEY_ESCAPE) => Action::SwitchBack,
                Event::Key(Key::PageDown) => Action::ScrollPageDown,
                Event::Key(Key::PageUp) => Action::ScrollPageUp,
                _ => Action::None,
            },
            KeyMap::Process => match evt {
                Event::Key(KEY_QUIT) | Event::Key(KEY_ESCAPE) => Action::SwitchBack,
                Event::Key(KEY_GOTO_TBL_BOTTOM) => Action::GotoTableBottom,
                Event::Key(KEY_GOTO_TBL_LEFT) => Action::GotoTableLeft,
                Event::Key(KEY_GOTO_TBL_RIGHT) => Action::GotoTableRight,
                Event::Key(KEY_GOTO_TBL_TOP) => Action::GotoTableTop,
                Event::Key(KEY_PAGE_LEFT) => Action::ScrollPageLeft,
                Event::Key(KEY_PAGE_RIGHT) => Action::ScrollPageRight,
                Event::Key(Key::PageDown) => Action::ScrollPageDown,
                Event::Key(Key::PageUp) => Action::ScrollPageUp,
                Event::Key(Key::Down) => Action::ScrollLineDown,
                Event::Key(Key::Up) => Action::ScrollLineUp,
                Event::Key(Key::Left) => Action::ScrollLeft,
                Event::Key(Key::Right) => Action::ScrollRight,
                _ => Action::None,
            },
            KeyMap::Details => match evt {
                Event::Key(KEY_QUIT) | Event::Key(KEY_ESCAPE) => Action::SwitchBack,
                Event::Key(KEY_SELECT_PARENT) => Action::SelectParent,
                Event::Key(KEY_LIMITS) => Action::SwitchToLimits,
                Event::Key(KEY_ENV) => Action::SwitchToEnvironment,
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
                Event::Key(KEY_PAGE_LEFT) => Action::ScrollPageLeft,
                Event::Key(KEY_PAGE_RIGHT) => Action::ScrollPageRight,
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
        MenuEntry::with_key(KEY_ENV, "Environment", KeyMapSet::OnlyIn(KeyMap::Details)),
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
    Previous,
    /// Select next search occurrence or mark.
    Next,
    /// Invert the marks of the matched lines or the current selection.
    ToggleMarks,
}

/// Action to edit search bar
#[derive(Debug, Clone, Copy)]
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
}

/// Search bar
#[derive(Debug, Default, Getters, Setters)]
pub struct Bookmarks {
    /// PID at the line under the cursor.
    #[getset(get = "pub")]
    selected_pid: Option<pid_t>,
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
    pub fn clear_search(&mut self) -> bool {
        match self.search {
            Some(_) => {
                self.search = None;
                true
            }
            None => false,
        }
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

    /// Select this PID.
    pub(crate) fn select_pid(&mut self, pid: pid_t) {
        self.selected_pid = Some(pid);
    }

    /// Transform the motion on the bookmarks to a motion on lines.
    ///
    /// Returns the selected line.
    ///
    /// * `scroll` - The requested scroll.
    /// * `occurrences` - The set of matching pid in case of search.
    /// * `lines` - The lines of process identities.
    pub(crate) fn selected_line<I, P>(
        &mut self,
        scroll: Scroll,
        occurrences: &mut BTreeSet<pid_t>,
        lines: I,
    ) -> Option<usize>
    where
        I: Iterator<Item = P>,
        P: ProcessIdentity,
    {
        occurrences.clear();
        let mut selected_linepid = None;
        let mut first_linepid = None;
        let mut last_linepid = None;
        let mut matches = Vec::new();
        let mut marks = Vec::new();
        let pattern = self.search_pattern();

        for (lineno, pi) in lines.enumerate() {
            let pid = pi.pid();
            if pid == 0 {
                continue;
            }
            let this_linepid = LinePid::new(lineno, pi.pid());
            if first_linepid.is_none() {
                first_linepid = last_linepid;
            }
            if self.marks.contains(&pid) {
                marks.push(this_linepid);
            }
            if let Some(selected_pid) = self.selected_pid {
                if selected_pid == pid {
                    selected_linepid = Some(this_linepid);
                }
            }
            if let Some(pattern) = pattern.as_ref() {
                if pi.name().contains(pattern) {
                    matches.push(this_linepid);
                    occurrences.insert(pid);
                }
            }
            last_linepid = Some(this_linepid);
        }
        match selected_linepid {
            Some(lp) if !occurrences.is_empty() && occurrences.contains(&lp.pid) => (),
            Some(lp) if !matches.is_empty() => {
                selected_linepid = self.move_to_pid_in_ring(&matches, Some(lp), LinePid::next_in);
            }
            _ if !matches.is_empty() => selected_linepid = matches.first().copied(),
            _ => (),
        }
        self.selected_pid = selected_linepid.map(|lp| lp.pid);

        self.marks = BTreeSet::from_iter(marks.iter().map(|lp| lp.pid)); // Keep only marks on existing PIDs.
        let ring = match pattern {
            Some(_) => &matches,
            None => &marks,
        };
        if selected_linepid.is_none() {
            self.selected_pid = None;
        }
        let action = self.action;
        self.action = BookmarkAction::None;
        let selected_linepid = match action {
            BookmarkAction::None | BookmarkAction::ToggleMarks => {
                if matches!(action, BookmarkAction::ToggleMarks) {
                    self.toggle_marks(occurrences);
                }
                self.apply_scroll(scroll, selected_linepid, first_linepid, last_linepid)
            }
            BookmarkAction::Previous => self
                .move_to_pid_in_ring(ring, selected_linepid, LinePid::previous_in)
                .or_else(|| ring.last().copied()),
            BookmarkAction::Next => self
                .move_to_pid_in_ring(ring, selected_linepid, LinePid::next_in)
                .or_else(|| ring.first().copied()),
        };
        self.selected_pid = selected_linepid.map(|lp| lp.pid);
        selected_linepid.map(|lp| lp.lineno)
    }

    /// Apply the scroll.
    fn apply_scroll(
        &mut self,
        scroll: Scroll,
        selected_linepid: Option<LinePid>,
        first_linepid: Option<LinePid>,
        last_linepid: Option<LinePid>,
    ) -> Option<LinePid> {
        match scroll {
            Scroll::FirstPosition => first_linepid,
            Scroll::LastPosition => last_linepid,
            _ => selected_linepid,
        }
    }

    /// Apply the function to the ring and pid if the latest is set.
    fn move_to_pid_in_ring<F>(
        &mut self,
        ring: &[LinePid],
        lp: Option<LinePid>,
        f: F,
    ) -> Option<LinePid>
    where
        F: for<'a> Fn(&LinePid, &'a [LinePid]) -> Option<&'a LinePid>,
    {
        lp.as_ref()
            .and_then(|lp| f(lp, ring).map(LinePid::to_owned))
    }

    /// Toggle marks in the given occurrences.
    fn toggle_marks(&mut self, occurrences: &mut BTreeSet<pid_t>) {
        if occurrences.is_empty() {
            if let Some(selected_pid) = self.selected_pid {
                self.toggle_mark(selected_pid);
            }
        } else {
            occurrences.iter().for_each(|pid| self.toggle_mark(*pid));
            self.clear_search();
            occurrences.clear();
        }
    }

    /// Toggle the mark for the given PID.
    fn toggle_mark(&mut self, pid: pid_t) {
        if !self.marks.remove(&pid) {
            self.marks.insert(pid);
        }
    }
}
