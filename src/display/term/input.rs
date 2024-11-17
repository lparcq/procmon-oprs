// Oprs -- process monitor for Linux
// Copyright (C) 2024  Laurent Pelecq
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

use bitmask_enum::bitmask;
use getset::{Getters, Setters};
use libc::pid_t;
use smart_default::SmartDefault;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    iter::IntoIterator,
};

use crate::{
    console::{Event, Key},
    process::ProcessIdentity,
};

use super::types::UnboundedSize;

/// Standard keys
const KEY_FASTER: Key = Key::Char(KEY_FASTER_CHAR);
const KEY_FASTER_CHAR: char = '+';
const KEY_FOCUS: Key = Key::Char('f');
const KEY_GOTO_TBL_BOTTOM: Key = Key::CtrlEnd;
const KEY_GOTO_TBL_LEFT: Key = Key::Home;
const KEY_GOTO_TBL_RIGHT: Key = Key::End;
const KEY_GOTO_TBL_TOP: Key = Key::CtrlHome;
const KEY_HELP: Key = Key::Char('?');
const KEY_LIMITS: Key = Key::Char('l');
const KEY_NEXT_FILTER: Key = Key::Char('F');
const KEY_SEARCH: Key = Key::Char('/');
const KEY_SEARCH_PREVIOUS_CHAR: char = 'N';
const KEY_SEARCH_PREVIOUS: Key = Key::Char(KEY_SEARCH_PREVIOUS_CHAR);
const KEY_SEARCH_NEXT_CHAR: char = 'n';
const KEY_SEARCH_NEXT: Key = Key::Char(KEY_SEARCH_NEXT_CHAR);
const KEY_SLOWER: Key = Key::Char(KEY_SLOWER_CHAR);
const KEY_SLOWER_CHAR: char = '-';
const KEY_QUIT: Key = Key::Esc;

/// User action
#[derive(Clone, Copy, Debug)]
pub enum Action {
    None,
    DivideTimeout(u16),
    FilterNext,
    Focus,
    GotoTableBottom,
    GotoTableLeft,
    GotoTableRight,
    GotoTableTop,
    HelpEnter,
    HelpExit,
    MultiplyTimeout(u16),
    Quit,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
    ScrollUp,
    SearchEnter,
    SearchExit,
    SearchPush(char),
    SearchPop,
    SearchPrevious,
    SearchNext,
    SelectDown,
    SelectUp,
    ToggleLimits,
}

/// Keymap
#[bitmask(u8)]
pub enum KeyMap {
    Main,
    Help,
    FixedSearch,
    IncrementalSearch,
}

impl KeyMap {
    /// Convert an input event to an action
    pub fn action_from_event(self, evt: Event) -> Action {
        if self.intersects(KeyMap::IncrementalSearch) {
            match evt {
                Event::Key(Key::Char('\n')) => Action::SearchExit,
                Event::Key(Key::Char(c)) => Action::SearchPush(c),
                Event::Key(Key::Backspace) => Action::SearchPop,
                _ => Action::None,
            }
        } else if self.intersects(KeyMap::Help) {
            match evt {
                Event::Key(KEY_QUIT) => Action::HelpExit,
                Event::Key(Key::PageDown) => Action::ScrollDown,
                Event::Key(Key::PageUp) => Action::ScrollUp,
                _ => Action::None,
            }
        } else {
            match evt {
                Event::Key(KEY_FASTER) => Action::DivideTimeout(2),
                Event::Key(KEY_FOCUS) => Action::Focus,
                Event::Key(KEY_GOTO_TBL_BOTTOM) => Action::GotoTableBottom,
                Event::Key(KEY_GOTO_TBL_LEFT) => Action::GotoTableLeft,
                Event::Key(KEY_GOTO_TBL_RIGHT) => Action::GotoTableRight,
                Event::Key(KEY_GOTO_TBL_TOP) => Action::GotoTableTop,
                Event::Key(KEY_HELP) => Action::HelpEnter,
                Event::Key(KEY_NEXT_FILTER) => Action::FilterNext,
                Event::Key(KEY_SEARCH) => Action::SearchEnter,
                Event::Key(KEY_SEARCH_PREVIOUS) if self.intersects(KeyMap::FixedSearch) => {
                    Action::SearchPrevious
                }
                Event::Key(KEY_SEARCH_NEXT) if self.intersects(KeyMap::FixedSearch) => {
                    Action::SearchNext
                }
                Event::Key(KEY_SLOWER) => Action::MultiplyTimeout(2),
                Event::Key(KEY_QUIT) | Event::Key(Key::Ctrl('c')) => Action::Quit,
                Event::Key(Key::PageDown) => Action::ScrollDown,
                Event::Key(Key::PageUp) => Action::ScrollUp,
                Event::Key(Key::Down) => Action::SelectDown,
                Event::Key(Key::Left) => Action::ScrollLeft,
                Event::Key(Key::Right) => Action::ScrollRight,
                Event::Key(Key::Up) => Action::SelectUp,
                Event::Key(KEY_LIMITS) => Action::ToggleLimits,
                _ => Action::None,
            }
        }
    }
}

/// Menu entry with a key and a label.
#[derive(Debug, Getters)]
pub struct MenuEntry {
    key: String,
    #[getset(get = "pub")]
    label: &'static str,
    pub keymap: KeyMap,
}

impl MenuEntry {
    pub fn new(key: String, label: &'static str, keymap: KeyMap) -> Self {
        Self { key, label, keymap }
    }

    pub fn with_key(key: Key, label: &'static str, keymap: KeyMap) -> Self {
        Self::new(MenuEntry::key_name(key), label, keymap)
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
            Key::Char(ch) => format!("{ch}"),
            Key::Alt(ch) => format!("M-{ch}"),
            Key::Ctrl(ch) => format!("C-{ch}"),
            Key::Null => "\\0".to_string(),
            KEY_QUIT => "Esc".to_string(),
            _ => "?".to_string(),
        }
    }
}

/// Return the menu
pub fn menu() -> Vec<MenuEntry> {
    vec![
        MenuEntry::with_key(
            KEY_QUIT,
            "Quit",
            KeyMap::Main | KeyMap::FixedSearch | KeyMap::Help,
        ),
        MenuEntry::with_key(KEY_HELP, "Help", KeyMap::Main),
        MenuEntry::new(
            format!(
                "{}/{}",
                MenuEntry::key_name(Key::Up),
                MenuEntry::key_name(Key::Down)
            ),
            "Select",
            KeyMap::Main,
        ),
        MenuEntry::new(
            format!("{KEY_SEARCH_NEXT_CHAR}/{KEY_SEARCH_PREVIOUS_CHAR}",),
            "Next/Prev",
            KeyMap::FixedSearch,
        ),
        MenuEntry::with_key(KEY_SEARCH, "Search", KeyMap::Main | KeyMap::FixedSearch),
        MenuEntry::with_key(KEY_LIMITS, "Limits", KeyMap::Main | KeyMap::FixedSearch),
        MenuEntry::with_key(KEY_FOCUS, "Focus", KeyMap::Main | KeyMap::FixedSearch),
        MenuEntry::with_key(
            KEY_NEXT_FILTER,
            "Filter",
            KeyMap::Main | KeyMap::FixedSearch,
        ),
        MenuEntry::new(
            format!("{KEY_FASTER_CHAR}/{KEY_SLOWER_CHAR}"),
            "Speed",
            KeyMap::Main | KeyMap::FixedSearch,
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
    /// Focus on the current selection
    Focus,
    /// Select previous line
    PreviousLine,
    /// Select next line
    NextLine,
    /// Select previous occurrence
    PreviousMatch,
    /// Select next occurrence
    NextMatch,
    /// Current selected line if it still matched, else the next matching.
    ClosestMatch,
}

#[derive(Clone, Copy, Debug)]
struct LinePid {
    lineno: usize,
    pid: pid_t,
}

impl LinePid {
    fn new(lineno: usize, pid: pid_t) -> Self {
        Self { lineno, pid }
    }
}

#[derive(Debug)]
struct LineContext {
    current: LinePid,
    previous_pid: Option<pid_t>,
    next_pid: Option<pid_t>,
}

impl LineContext {
    fn new(lineno: usize, current: pid_t, previous_pid: Option<pid_t>) -> Self {
        Self {
            current: LinePid::new(lineno, current),
            previous_pid,
            next_pid: None,
        }
    }
}

/// Matches occurrences of a predicate and the previous and next occurrences of a selection.
#[derive(Debug, Default)]
struct LineMatcher {
    first: Option<LineContext>,
    occurrences: Vec<LineContext>,
    pids: BTreeMap<pid_t, usize>,
}

impl LineMatcher {
    /// Return the context for an optional PID.
    fn context_map<F>(&self, pid: Option<pid_t>, func: F) -> Option<&LineContext>
    where
        F: Fn(usize) -> usize,
    {
        match pid.and_then(|pid| {
            self.pids
                .get(&pid)
                .map(|n| func(*n) % self.occurrences.len())
        }) {
            Some(n) => self.occurrences.get(n),
            None => self.occurrences.first().or(self.first.as_ref()),
        }
    }

    /// Return the context for an optional PID.
    fn context(&self, pid: Option<pid_t>) -> Option<&LineContext> {
        self.context_map(pid, |n| n)
    }

    /// Return the context at optional pid.
    fn current_pid(&self, pid: Option<pid_t>) -> Option<LinePid> {
        self.context_map(pid, |n| n).map(|c| c.current)
    }

    /// Return the context after optional PID.
    fn next_pid(&self, pid: Option<pid_t>) -> Option<LinePid> {
        self.context_map(pid, |n| n + 1).map(|c| c.current)
    }

    /// Return the context after optional PID.
    fn previous_pid(&self, pid: Option<pid_t>) -> Option<LinePid> {
        let len = self.occurrences.len();
        self.context_map(pid, |n| n + len - 1).map(|c| c.current)
    }

    /// Return the given pids only if it's an occurrence.
    fn matching_pid(&self, pid_hint: Option<pid_t>) -> Option<pid_t> {
        pid_hint.and_then(|pid| {
            if self.pids.contains_key(&pid) {
                Some(pid)
            } else {
                None
            }
        })
    }

    /// Take the first line as a match.
    fn take_first<I, P>(&mut self, lines: I)
    where
        I: Iterator<Item = P>,
        P: ProcessIdentity,
    {
        lines.enumerate().take(1).for_each(|(lineno, pi)| {
            self.first = Some(LineContext::new(lineno, pi.pid(), None));
        });
    }

    /// Find all lines that match a predicate.
    fn find<I, F, P>(&mut self, lines: I, pred: F)
    where
        I: Iterator<Item = P>,
        F: Fn(&P) -> bool,
        P: ProcessIdentity,
    {
        let mut previous_pid = None;
        let mut next_lineno = usize::MAX;
        lines.into_iter().enumerate().for_each(|(lineno, pi)| {
            let pid = pi.pid();
            if self.first.is_none() {
                self.first = Some(LineContext::new(lineno, pid, None));
            }
            if lineno == next_lineno {
                if let Some(last) = self.occurrences.last_mut() {
                    last.next_pid = Some(pid);
                }
            }
            if pred(&pi) {
                let context = LineContext::new(lineno, pid, previous_pid);
                next_lineno = lineno + 1;
                self.pids.insert(pid, self.occurrences.len());
                self.occurrences.push(context);
            }
            previous_pid = Some(pid)
        })
    }
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

#[derive(Debug)]
pub enum PidStatus {
    Unknown,
    Selected,
    Matching,
}

/// Search bar
#[derive(Debug, Default, Getters, Setters)]
pub struct Bookmarks {
    #[getset(get = "pub")]
    selected: Option<pid_t>,
    #[getset(get = "pub")]
    occurrences: BTreeSet<pid_t>,
    #[getset(get = "pub")]
    search: Option<SearchBar>,
    #[getset(get = "pub", set = "pub")]
    action: BookmarkAction,
}

impl Bookmarks {
    /// Recenter vertically on a given position.
    ///
    /// # Arguments
    ///
    /// * `center` - The position to recenter to.
    /// * `height` - The height of the visible area.
    /// * `force` - Recenter even if the position is already visible.
    fn recenter(center: usize, low: UnboundedSize, height: usize, force: bool) -> Option<usize> {
        let ucenter = UnboundedSize::Value(center);
        let high = low + UnboundedSize::Value(height);
        if force || ucenter < low || ucenter >= high {
            Some(center.saturating_sub(std::cmp::max(1, height / 2)))
        } else {
            None
        }
    }

    /// Status of a PID.
    pub fn status(&self, pid: pid_t) -> PidStatus {
        if self.selected == Some(pid) {
            PidStatus::Selected
        } else if self.occurrences.contains(&pid) {
            PidStatus::Matching
        } else {
            PidStatus::Unknown
        }
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

    /// Whether there is an ongoing search.
    pub fn is_search(&self) -> bool {
        self.search.is_some()
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

    /// Recenter on the currently selected line or the first.
    fn execute_focus(
        &mut self,
        matcher: LineMatcher,
        top: UnboundedSize,
        height: usize,
    ) -> (Option<usize>, Option<usize>) {
        match matcher.context(self.selected) {
            Some(LineContext { current, .. }) => {
                let lineno = current.lineno;
                self.selected = Some(current.pid);
                (Some(lineno), Bookmarks::recenter(lineno, top, height, true))
            }
            None => (None, None),
        }
    }

    /// Recenter on the line before the currently selected line.
    fn execute_previous_line(
        &mut self,
        matcher: LineMatcher,
        top: UnboundedSize,
        height: usize,
    ) -> (Option<usize>, Option<usize>) {
        match matcher.context(self.selected) {
            Some(LineContext {
                current,
                previous_pid: Some(previous_pid),
                ..
            }) => {
                let lineno = current.lineno - 1;
                let center = Bookmarks::recenter(lineno, top, height, false);
                self.selected = Some(*previous_pid);
                (Some(lineno), center)
            }
            Some(LineContext {
                current,
                previous_pid: None,
                ..
            }) => {
                let lineno = current.lineno;
                self.selected = Some(current.pid);
                (Some(lineno), Bookmarks::recenter(lineno, top, height, true))
            }
            _ => {
                self.selected = None;
                (None, None)
            }
        }
    }

    /// Recenter on the line after the currently selected line.
    fn execute_next_line(
        &mut self,
        matcher: LineMatcher,
        top: UnboundedSize,
        height: usize,
    ) -> (Option<usize>, Option<usize>) {
        match matcher.context(self.selected) {
            Some(LineContext {
                current,
                previous_pid: _,
                next_pid: Some(next_pid),
            }) => {
                let lineno = current.lineno + 1;
                let center = Bookmarks::recenter(lineno, top, height, false);
                self.selected = Some(*next_pid);
                (Some(lineno), center)
            }
            Some(LineContext {
                current,
                previous_pid: _,
                next_pid: None,
            }) => {
                let lineno = current.lineno;
                self.selected = Some(current.pid);
                (Some(lineno), Bookmarks::recenter(lineno, top, height, true))
            }
            _ => {
                self.selected = None;
                (None, None)
            }
        }
    }

    /// Recenter on the matching line before the currently selected line.
    fn execute_previous_match(
        &mut self,
        matcher: LineMatcher,
        top: UnboundedSize,
        height: usize,
    ) -> (Option<usize>, Option<usize>) {
        match matcher.previous_pid(matcher.matching_pid(self.selected)) {
            Some(LinePid { lineno, pid }) => {
                self.selected = Some(pid);
                (
                    Some(lineno),
                    Bookmarks::recenter(lineno, top, height, false),
                )
            }
            None => (None, None),
        }
    }

    /// Recenter on the matching line before the currently selected line.
    fn execute_next_match(
        &mut self,
        matcher: LineMatcher,
        top: UnboundedSize,
        height: usize,
    ) -> (Option<usize>, Option<usize>) {
        match matcher.next_pid(matcher.matching_pid(self.selected)) {
            Some(LinePid { lineno, pid }) => {
                self.selected = Some(pid);
                (
                    Some(lineno),
                    Bookmarks::recenter(lineno, top, height, false),
                )
            }
            None => (None, None),
        }
    }

    /// Recenter on the matching line before the currently selected line.
    fn execute_closest_match(
        &mut self,
        matcher: LineMatcher,
        top: UnboundedSize,
        height: usize,
    ) -> (Option<usize>, Option<usize>) {
        match matcher.current_pid(matcher.matching_pid(self.selected)) {
            Some(LinePid { lineno, pid }) => {
                self.selected = Some(pid);
                (
                    Some(lineno),
                    Bookmarks::recenter(lineno, top, height, false),
                )
            }
            None => (None, None),
        }
    }

    fn clear_occurrences(&mut self) {
        self.occurrences.clear();
        if let Some(selected) = self.selected {
            self.occurrences.insert(selected);
        }
    }

    /// Execute the action and return the selected line and the vertical offset.
    pub fn execute<I, P>(
        &mut self,
        lines: I,
        top: UnboundedSize,
        height: usize,
    ) -> (Option<usize>, Option<usize>)
    where
        I: Iterator<Item = P>,
        P: ProcessIdentity,
    {
        let mut matcher = LineMatcher::default();
        let action = self.action;
        self.action = match self.search {
            Some(_) => BookmarkAction::ClosestMatch,
            None => BookmarkAction::None,
        };
        match action {
            BookmarkAction::None
            | BookmarkAction::Focus
            | BookmarkAction::PreviousLine
            | BookmarkAction::NextLine => match self.selected {
                Some(pid) => matcher.find(lines, |pi| pi.pid() == pid),
                None => matcher.take_first(lines),
            },
            BookmarkAction::PreviousMatch
            | BookmarkAction::NextMatch
            | BookmarkAction::ClosestMatch => match self.search.as_ref().map(|s| s.pattern()) {
                Some(ref pattern) if !pattern.is_empty() => {
                    matcher.find(lines, |pi| pi.name().contains(pattern));
                }
                _ => (),
            },
        };
        self.occurrences = BTreeSet::from_iter(matcher.occurrences.iter().map(|lc| lc.current.pid));
        match action {
            BookmarkAction::None => (None, None),
            BookmarkAction::Focus => self.execute_focus(matcher, top, height),
            BookmarkAction::PreviousLine => {
                let result = self.execute_previous_line(matcher, top, height);
                self.clear_occurrences();
                result
            }
            BookmarkAction::NextLine => {
                let result = self.execute_next_line(matcher, top, height);
                self.clear_occurrences();
                result
            }
            BookmarkAction::PreviousMatch => self.execute_previous_match(matcher, top, height),
            BookmarkAction::NextMatch => self.execute_next_match(matcher, top, height),
            BookmarkAction::ClosestMatch => self.execute_closest_match(matcher, top, height),
        }
    }
}
