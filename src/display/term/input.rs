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
use std::{
    collections::{BTreeSet, HashMap},
    fmt,
    rc::Rc,
};

use crate::{
    console::{Event, Key},
    process::ProcessIdentity,
};

use super::types::Scroll;

/// Standard keys
const KEY_ABOUT: Key = Key::Char('a');
const KEY_ENTER: Key = Key::Char('\n');
const KEY_ENV: Key = Key::Char('e');
const KEY_ESCAPE: Key = Key::Esc;
const KEY_FASTER: Key = Key::Char(KEY_FASTER_CHAR);
const KEY_FASTER_CHAR: char = '+';
const KEY_FILES: Key = Key::Char('f');
const KEY_FILTER_ACTIVE: Key = Key::Char('a');
const KEY_FILTER_NONE: Key = Key::Char('n');
const KEY_FILTER_USERS: Key = Key::Char('u');
const KEY_FILTER_CURRENT_USER: Key = Key::Char('m');
const KEY_GOTO_TBL_BOTTOM: Key = Key::CtrlEnd;
const KEY_GOTO_TBL_LEFT: Key = Key::Home;
const KEY_GOTO_TBL_RIGHT: Key = Key::End;
const KEY_GOTO_TBL_TOP: Key = Key::CtrlHome;
const KEY_HELP: Key = Key::Char('?');
const KEY_LIMITS: Key = Key::Char('l');
const KEY_MAPS: Key = Key::Char('m');
const KEY_MARK_CLEAR: Key = Key::Ctrl('c');
const KEY_MARK_TOGGLE: Key = Key::Char(' ');
const KEY_MENU_HELP: Key = Key::F(1);
const KEY_MENU_EDIT: Key = Key::F(2);
const KEY_MENU_NAVIGATE: Key = Key::F(3);
const KEY_MENU_SEARCH: Key = Key::F(5);
const KEY_MENU_SELECT: Key = Key::F(6);
const KEY_MENU_FILTER: Key = Key::F(7);
const KEY_PAGE_LEFT: Key = Key::BackTab;
const KEY_PAGE_RIGHT: Key = Key::Char('\t');
const KEY_QUIT: Key = Key::Char('q');
const KEY_SCOPE: Key = Key::Char('s');
const KEY_SEARCH: Key = Key::Char('/');
const KEY_SEARCH_CANCEL: Key = Key::Ctrl('c');
const KEY_SEARCH_NEXT: Key = Key::Ctrl('n');
const KEY_SEARCH_PREVIOUS: Key = Key::Ctrl('N');
const KEY_SELECT_NEXT: Key = Key::Char('n');
const KEY_SELECT_PREVIOUS: Key = Key::Char('N');
const KEY_SELECT_PARENT: Key = Key::Char('p');
const KEY_SELECT_ROOT_PID: Key = Key::Char('r');
const KEY_UNSELECT_ROOT_PID: Key = Key::Char('R');
const KEY_SLOWER: Key = Key::Char(KEY_SLOWER_CHAR);
const KEY_SLOWER_CHAR: char = '-';

/// User action
#[derive(Clone, Copy, Debug, SmartDefault)]
pub enum Action {
    #[default]
    None,
    ChangeScope,
    DivideTimeout(u16),
    FilterNone,
    FilterUsers,
    FilterActive,
    FilterCurrentUser,
    GotoTableBottom,
    GotoTableLeft,
    GotoTableRight,
    GotoTableTop,
    SwitchToAbout,
    SwitchToHelp,
    SwitchBack,
    SwitchToDetails,
    SwitchToLimits,
    SwitchToEnvironment,
    SwitchToFiles,
    SwitchToMaps,
    ClearMarks,
    ToggleMarks,
    MultiplyTimeout(u16),
    PushChar(char),
    PopChar,
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
    SelectNext,
    SelectPrevious,
    SelectParent,
    SelectRootPid,
    UnselectRootPid,
}

impl Action {
    /// True if not None
    pub fn is_some(&self) -> bool {
        !matches!(self, Action::None)
    }

    /// Convert an action to an Option.
    pub fn ok(self) -> Option<Action> {
        match self {
            Action::None => None,
            _ => Some(self),
        }
    }

    /// True if the action implies reverting to the parent menu
    pub fn parent_menu(&self) -> bool {
        matches!(self, Self::SearchExit | Self::SwitchBack)
    }
}

/// Menu target
#[derive(Debug)]
pub enum MenuTarget {
    // Action
    Action(Action),
    // Sub-menu
    Menu(Rc<Menu>),
}

impl Clone for MenuTarget {
    fn clone(&self) -> Self {
        match self {
            Self::Action(action) => Self::Action(*action),
            Self::Menu(menu) => Self::Menu(Rc::clone(menu)),
        }
    }
}

/// Menu entry with a key and a label.
#[derive(Debug, Getters)]
pub struct MenuEntry {
    #[getset(get = "pub")]
    key: String,
    #[getset(get = "pub")]
    label: &'static str,
}

impl MenuEntry {
    fn new(key: Key, label: &'static str) -> Self {
        let key = Self::key_name(key);
        Self { key, label }
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
            Key::Char('\n') => "⏎".to_string(),
            Key::Char(' ') => "Spc".to_string(),
            Key::Char(ch) => format!("{ch}"),
            Key::Alt(ch) => format!("M-{ch}"),
            Key::Ctrl(ch) => format!("C-{ch}"),
            Key::Null => "\\0".to_string(),
            _ => "?".to_string(),
        }
    }
}

/// A menu with entries to display and shorcuts.
///
/// A shorcut directly executes an action or opens a sub-menu whether it's
/// displayed or not.
#[derive(Debug, Default)]
pub struct Menu {
    /// Menu name
    pub name: &'static str,
    /// The action associated with this menu.
    pub action: Action,
    /// The list of entries to display.
    entries: Vec<MenuEntry>,
    /// The targets for each menu entries.
    targets: HashMap<Key, MenuTarget>,
    /// The direct actions without menu entries.
    shortcuts: HashMap<Key, Action>,
    /// Whether the menu accept any char.
    char_stream: bool,
}

impl Menu {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            action: Action::None,
            ..Default::default()
        }
    }
    /// Return a slice iterator to the menu entries.
    pub fn entries(&self) -> std::slice::Iter<MenuEntry> {
        self.entries.iter()
    }

    /// Get the target
    pub fn map_key(&self, key: &Key) -> Option<MenuTarget> {
        self.targets
            .get(key)
            .cloned()
            .or_else(|| self.shortcuts.get(key).map(|a| MenuTarget::Action(*a)))
            .or({
                if self.char_stream {
                    match key {
                        Key::Char(c) => Some(MenuTarget::Action(Action::PushChar(*c))),
                        Key::Backspace => Some(MenuTarget::Action(Action::PopChar)),
                        _ => None,
                    }
                } else {
                    log::debug!("menu {}: unknown key {:?}", self.name, key);
                    None
                }
            })
    }

    /// Get the target from an event.
    pub fn map_event(&self, evt: Event) -> Option<MenuTarget> {
        match evt {
            Event::Key(key) => self.map_key(&key),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct MenuBuilder(Rc<Menu>);

impl MenuBuilder {
    fn new(name: &'static str) -> Self {
        Self(Rc::new(Menu::new(name)))
    }

    fn menu(&mut self) -> &mut Menu {
        Rc::get_mut(&mut self.0).expect("no reference to menu when constructed")
    }

    /// Add a target.
    fn with_target(mut self, key: Key, label: &'static str, target: MenuTarget) -> Self {
        let menu = self.menu();
        menu.entries.push(MenuEntry::new(key, label));
        menu.targets.insert(key, target);
        self
    }

    /// Add an action.
    fn with_action(self, key: Key, label: &'static str, action: Action) -> Self {
        self.with_target(key, label, MenuTarget::Action(action))
    }

    /// Add a sub-menu.
    fn with_menu(self, key: Key, menu: Rc<Menu>) -> Self {
        self.with_target(key, menu.name, MenuTarget::Menu(menu))
    }

    /// Add a sub-menu and import all its entries as shortcuts.
    fn with_imported_entries(self, key: Key, menu: Rc<Menu>) -> Self {
        self.import_actions(&menu).with_menu(key, menu)
    }

    /// Add a shortcut.
    ///
    /// The shortcut is not visible in the menu.
    fn with_shortcut(mut self, key: Key, action: Action) -> Self {
        self.menu().shortcuts.insert(key, action);
        self
    }

    /// Import submenu actions as shortcuts.
    fn import_actions(mut self, other: &Menu) -> Self {
        let menu = self.menu();
        // Import the menu entries with a self action in the targets.
        menu.targets
            .extend(other.targets.iter().filter_map(|(key, tgt)| match tgt {
                MenuTarget::Menu(menu) if menu.action.is_some() => Some((*key, tgt.clone())),
                _ => None,
            }));
        // Import the menu entries actions as shortcuts.
        menu.shortcuts
            .extend(other.targets.iter().filter_map(|(key, tgt)| match tgt {
                MenuTarget::Action(action) => Some((key, action)),
                MenuTarget::Menu(_) => None,
            }));
        self
    }

    /// Import menu shortcuts.
    fn import_shortcuts(mut self, other: &Menu) -> Self {
        self.menu().shortcuts.extend(other.shortcuts.iter());
        self
    }

    /// Accept any char.
    fn enable_char_stream(mut self) -> Self {
        self.menu().char_stream = true;
        self
    }

    /// Associate an interaction
    fn with_self_action(mut self, action: Action) -> Self {
        self.menu().action = action;
        self
    }

    /// Build the menu
    fn build(self) -> Rc<Menu> {
        let Self(menu) = self;
        menu
    }

    /// Duplicate a menu with a different self actions.
    fn duplicate(name: &'static str, menu: &Menu, action: Action) -> Rc<Menu> {
        MenuBuilder::new(name)
            .import_actions(menu)
            .import_shortcuts(menu)
            .with_self_action(action)
            .build()
    }
}

/// Return the menu
pub fn menu() -> Rc<Menu> {
    let menu_help = MenuBuilder::new("Help")
        .with_action(KEY_HELP, "Help", Action::SwitchToHelp)
        .with_action(KEY_ABOUT, "About", Action::SwitchToAbout)
        .with_action(KEY_QUIT, "Quit", Action::Quit)
        .with_shortcut(KEY_ESCAPE, Action::SwitchBack)
        .build();
    let menu_edit = MenuBuilder::new("Edit")
        .with_action(KEY_FASTER, "Faster", Action::DivideTimeout(2))
        .with_action(KEY_SLOWER, "Slower", Action::MultiplyTimeout(2))
        .with_shortcut(KEY_ESCAPE, Action::SwitchBack)
        //.new(KEY_with_action, "Settings", Action::SwitchToSettings))
        .build();
    let menu_nav = MenuBuilder::new("Navigate")
        .with_action(Key::Up, "Previous line", Action::ScrollLineUp)
        .with_action(Key::Down, "Next line", Action::ScrollLineDown)
        .with_action(Key::Left, "Scroll left", Action::ScrollLeft)
        .with_action(Key::Right, "Scroll right", Action::ScrollRight)
        .with_action(Key::PageUp, "Page Up", Action::ScrollPageUp)
        .with_action(Key::PageDown, "Page Down", Action::ScrollPageDown)
        .with_action(KEY_PAGE_LEFT, "Page Left", Action::ScrollPageLeft)
        .with_action(KEY_PAGE_RIGHT, "Page Right", Action::ScrollPageRight)
        .with_action(KEY_GOTO_TBL_TOP, "Table Top", Action::GotoTableTop)
        .with_action(KEY_GOTO_TBL_BOTTOM, "Table Bottom", Action::GotoTableBottom)
        .with_action(KEY_GOTO_TBL_LEFT, "Table Left", Action::GotoTableLeft)
        .with_action(KEY_GOTO_TBL_RIGHT, "Table Right", Action::GotoTableRight)
        .with_shortcut(KEY_ESCAPE, Action::SwitchBack)
        .build();
    let menu_process_env =
        MenuBuilder::duplicate("Environment", &menu_nav, Action::SwitchToEnvironment);
    let menu_process_files = MenuBuilder::duplicate("Files", &menu_nav, Action::SwitchToFiles);
    let menu_process_limits = MenuBuilder::duplicate("Limits", &menu_nav, Action::SwitchToLimits);
    let menu_process_maps = MenuBuilder::duplicate("Maps", &menu_nav, Action::SwitchToMaps);
    let menu_details = MenuBuilder::new("Details")
        .with_menu(KEY_ENV, menu_process_env)
        .with_menu(KEY_FILES, menu_process_files)
        .with_menu(KEY_LIMITS, menu_process_limits)
        .with_menu(KEY_MAPS, menu_process_maps)
        .import_actions(&menu_nav)
        .import_shortcuts(&menu_nav)
        .with_self_action(Action::SwitchToDetails)
        .build();
    let menu_isearch = MenuBuilder::new("Incremental Search")
        .enable_char_stream()
        .with_shortcut(KEY_ENTER, Action::SearchExit)
        .with_shortcut(KEY_SEARCH_NEXT, Action::SelectNext)
        .with_shortcut(KEY_SEARCH_PREVIOUS, Action::SelectPrevious)
        .with_shortcut(KEY_SEARCH_CANCEL, Action::SearchCancel)
        .with_self_action(Action::SearchEnter)
        .build();
    let menu_search = MenuBuilder::new("Search")
        .with_menu(KEY_SEARCH, menu_isearch)
        .with_action(KEY_MARK_CLEAR, "Clear", Action::ClearMarks)
        .with_action(
            KEY_SELECT_PREVIOUS,
            "Previous Match",
            Action::SelectPrevious,
        )
        .with_action(KEY_SELECT_NEXT, "Next Match", Action::SelectNext)
        .with_shortcut(KEY_ESCAPE, Action::SwitchBack)
        .build();
    let menu_filter = MenuBuilder::new("Filters")
        .with_action(KEY_FILTER_NONE, "None", Action::FilterNone)
        .with_action(KEY_FILTER_USERS, "Users", Action::FilterUsers)
        .with_action(KEY_FILTER_ACTIVE, "Active", Action::FilterActive)
        .with_action(
            KEY_FILTER_CURRENT_USER,
            "Current User",
            Action::FilterCurrentUser,
        )
        .with_shortcut(KEY_ESCAPE, Action::SwitchBack)
        .build();
    let menu_select = MenuBuilder::new("Select")
        .with_menu(KEY_ENTER, menu_details)
        .with_action(KEY_MARK_TOGGLE, "Mark", Action::ToggleMarks)
        .with_action(KEY_MARK_CLEAR, "Clear", Action::ClearMarks)
        .with_action(KEY_SCOPE, "Scope", Action::ChangeScope)
        .with_action(KEY_SELECT_ROOT_PID, "Root", Action::SelectRootPid)
        .with_action(
            KEY_UNSELECT_ROOT_PID,
            "Unselect Root",
            Action::UnselectRootPid,
        )
        .with_action(KEY_SELECT_PARENT, "Parent", Action::SelectParent)
        .with_shortcut(KEY_ESCAPE, Action::SwitchBack)
        .build();
    MenuBuilder::new("Main")
        .with_menu(KEY_MENU_HELP, menu_help)
        .with_imported_entries(KEY_MENU_EDIT, menu_edit)
        .with_imported_entries(KEY_MENU_NAVIGATE, menu_nav)
        .with_imported_entries(KEY_MENU_SEARCH, menu_search)
        .with_imported_entries(KEY_MENU_SELECT, menu_select)
        .with_menu(KEY_MENU_FILTER, menu_filter)
        .with_shortcut(KEY_HELP, Action::SwitchToHelp)
        .with_shortcut(KEY_QUIT, Action::Quit)
        .with_shortcut(KEY_ESCAPE, Action::Quit)
        .build()
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
        if !matches!(scroll, Scroll::CurrentPosition) {
            self.search = None;
            self.clear_marks();
        }
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

#[cfg(test)]
mod tests {

    use super::{Action, KEY_HELP, KEY_MENU_HELP, Key, MenuBuilder, MenuTarget, menu};

    #[test]
    fn test_menu_not_found() {
        let menu = menu();
        let entry = dbg!(menu.map_key(&Key::Null));
        assert!(entry.is_none());
    }

    #[test]
    fn test_menu_action() {
        let menu = menu();
        match menu.map_key(&KEY_MENU_HELP).unwrap() {
            MenuTarget::Menu(menu_help) => {
                let entry = dbg!(menu_help.map_key(&KEY_HELP));
                assert!(matches!(
                    entry,
                    Some(MenuTarget::Action(Action::SwitchToHelp))
                ));
            }
            MenuTarget::Action(_) => panic!("got an action instead of a menu"),
        }
    }

    #[test]
    fn test_menu_submenu() {
        let menu = menu();
        let entry = dbg!(menu.map_key(&KEY_MENU_HELP));
        assert!(matches!(entry, Some(MenuTarget::Menu(_))));
    }

    #[test]
    fn test_duplicate_menu() {
        // Create a menu with a submenu without self action
        let key_submenu_action_a = Key::Char('a');
        let submenu_wo_action = MenuBuilder::new("SubmenuNoAction")
            .with_action(key_submenu_action_a, "Action A", Action::ToggleMarks)
            .build();

        // Create a menu with a submenu with self action
        let key_submenu_action_b = Key::Char('b');
        let submenu_wi_action = MenuBuilder::new("SubmenuWithAction")
            .with_action(key_submenu_action_b, "Action B", Action::ClearMarks)
            .with_self_action(Action::SelectNext)
            .build();

        // Create a main menu with both submenus, an action, and a shortcut
        let key_menu_wo_action = Key::Char('1');
        let key_menu_wi_action = Key::Char('2');
        let key_action = Key::Char('c');
        let key_shortcut = Key::Char('s');
        let original_menu = MenuBuilder::new("OriginalMenu")
            .with_menu(key_menu_wo_action, submenu_wo_action)
            .with_menu(key_menu_wi_action, submenu_wi_action)
            .with_action(key_action, "Action C", Action::SelectPrevious)
            .with_shortcut(key_shortcut, Action::Quit)
            .build();

        // Create a duplicate menu with a different self action
        let duplicated_menu =
            MenuBuilder::duplicate("DuplicatedMenu", &original_menu, Action::ChangeScope);

        // Action is set
        assert!(matches!(duplicated_menu.action, Action::ChangeScope));

        // Menu without actions are ignored
        assert!(duplicated_menu.map_key(&key_menu_wo_action).is_none());

        // Menu with an action are kept as target
        match duplicated_menu.map_key(&key_menu_wi_action) {
            Some(MenuTarget::Menu(menu)) => {
                assert!(matches!(dbg!(menu.action), Action::SelectNext))
            }
            Some(MenuTarget::Action(_)) => panic!("action instead of menu"),
            None => panic!("unknown key"),
        }

        // Actions are duplicated
        assert!(matches!(
            duplicated_menu.map_key(&key_action),
            Some(MenuTarget::Action(Action::SelectPrevious))
        ));

        // Shortcuts are duplicated
        assert!(matches!(
            duplicated_menu.map_key(&key_shortcut),
            Some(MenuTarget::Action(Action::Quit))
        ));

        // Sub-entries are not duplicated
        assert!(matches!(
            duplicated_menu.map_key(&key_submenu_action_a),
            None
        ));
        assert!(matches!(
            duplicated_menu.map_key(&key_submenu_action_b),
            None
        ));
    }
}
