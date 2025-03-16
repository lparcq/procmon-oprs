// Oprs -- process monitor for Linux
// Copyright (C) 2020-2025  Laurent Pelecq
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

use chrono::Local;
use libc::pid_t;
use ratatui::{
    backend::TermionBackend, prelude::*, style::Style, text::Text, widgets::Clear, Terminal,
};
use std::{cell::RefCell, convert::TryFrom, fmt, io, rc::Rc, time::Duration};
use termion::{
    raw::{IntoRawMode, RawTerminal},
    screen::{AlternateScreen, IntoAlternateScreen},
};

use crate::{
    clock::Timer,
    console::{is_tty, theme::BuiltinTheme, EventChannel},
    process::{
        self, format::human_duration, Aggregation, Collector, FormattedMetric, Process,
        ProcessDetails, ProcessFilter,
    },
};

use super::{DataKind, DisplayDevice, PaneData, PaneKind, PauseStatus, SliceIter};

mod input;
mod panes;
mod tables;

#[macro_use]
mod types;

use input::{menu, Action, BookmarkAction, Bookmarks, KeyMap, MenuEntry, SearchEdit};
use panes::{
    BigTableState, BigTableWidget, FieldsWidget, GridPane, MarkdownWidget, OneLineWidget,
    OptionalRenderer, Pane, SingleScrollablePane, TableGenerator, TableStyle, Zoom,
};
use tables::{
    EnvironmentTable, FilesTable, LimitsTable, MapsTable, ProcessTreeTable, Styles, TreeData,
};
use types::{Area, Motion};

const HELP: &str = include_str!("help_en.md");

/// User action that has an impact on the application.
#[derive(Clone, Debug)]
pub enum Interaction {
    None,
    Filter(ProcessFilter),
    SwitchBack,
    SwitchToHelp,
    SwitchTo(DataKind),
    SelectPid(pid_t),
    SelectParent,
    SelectRootPid(Option<pid_t>),
    Narrow(Vec<pid_t>),
    Wide,
    Quit,
}

impl TryFrom<&Action> for Interaction {
    type Error = ();

    /// Convert actions that have a one to one correspondance.
    fn try_from(value: &Action) -> Result<Self, Self::Error> {
        match value {
            Action::SelectParent => Ok(Interaction::SelectParent),
            Action::SwitchToHelp => Ok(Interaction::SwitchToHelp),
            Action::SwitchBack => Ok(Interaction::SwitchBack),
            Action::Quit => Ok(Interaction::Quit),
            _ => Err(()),
        }
    }
}

macro_rules! format_metric {
    ($metrics:expr, $field:ident) => {
        TerminalDevice::format_option($metrics.as_ref().and_then(|m| m.$field.strings().next()))
    };
}

type Screen = AlternateScreen<RawTerminal<io::Stdout>>;
type TermionTerminal = Terminal<TermionBackend<Box<Screen>>>;

/// Print on standard output as a table
pub struct TerminalDevice {
    /// Interval to update the screen
    every: Duration,
    /// Channel for input events
    events: EventChannel,
    /// Terminal
    terminal: RefCell<TermionTerminal>,
    /// Table tree data
    tree_data: Rc<TreeData>,
    /// Position in the panes. Last position for the currently visible pane.
    motions: Vec<Area<Motion>>,
    /// Filter
    filter: ProcessFilter,
    /// Menu
    menu: Vec<MenuEntry>,
    /// Pane kind.
    pane_kind: PaneKind,
    /// Key map
    keymap: KeyMap,
}

impl TerminalDevice {
    pub fn new(every: Duration, theme: Option<BuiltinTheme>) -> anyhow::Result<Self> {
        let screen = io::stdout().into_raw_mode()?.into_alternate_screen()?;
        let backend = TermionBackend::new(Box::new(screen));
        let terminal = RefCell::new(Terminal::new(backend)?);

        Ok(TerminalDevice {
            every,
            events: EventChannel::new(),
            terminal,
            tree_data: Rc::new(TreeData::new(Styles::new(theme))),
            motions: vec![Area::default()],
            filter: ProcessFilter::default(),
            menu: menu(),
            pane_kind: PaneKind::Main,
            keymap: KeyMap::Main,
        })
    }

    pub fn is_available() -> bool {
        is_tty(&io::stdin())
    }

    /// Set the keymap
    fn set_keymap(&mut self, keymap: KeyMap) {
        if self.keymap != keymap {
            log::debug!("switch keymap from {} to {keymap}", self.keymap);
            self.keymap = keymap;
        }
    }

    /// Content of the status bar
    fn status_bar(&self) -> String {
        let time_string = format!("{}", Local::now().format("%X"));
        let delay = human_duration(self.every);
        let matches_count = self.tree_data.occurrences.len();
        let marks_count = self.tree_data.bookmarks.marks().len();
        if matches_count > 0 {
            format!("{time_string} -- interval:{delay} -- matches:{matches_count}",)
        } else if marks_count > 0 {
            format!("{time_string} -- interval:{delay} -- marks:{marks_count}",)
        } else {
            format!(
                "{time_string} -- interval:{delay} -- filter:{}",
                self.filter
            )
        }
    }

    /// Apply a function on bookmarks.
    fn on_bookmarks<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut Bookmarks),
    {
        match Rc::get_mut(&mut self.tree_data) {
            Some(data) => f(&mut data.bookmarks),
            None => log::error!("cannot clear bookmarks"),
        }
    }

    /// Clear marks.
    fn clear_bookmarks(&mut self) {
        self.on_bookmarks(|bookmarks| {
            if !bookmarks.clear_search() {
                bookmarks.clear_marks();
            }
        });
    }

    /// Clear search.
    fn clear_search(&mut self) {
        self.on_bookmarks(|bookmarks| void!(bookmarks.clear_search()));
    }

    /// Edit search.
    fn edit_search(&mut self, edit: SearchEdit) {
        self.on_bookmarks(|bookmarks| void!(bookmarks.edit_search(edit)));
    }

    /// Set bookmark action.
    fn set_bookmarks_action(&mut self, action: BookmarkAction) {
        self.on_bookmarks(|bookmarks| void!(bookmarks.set_action(action)));
    }

    fn last_motions(&mut self) -> &mut Area<Motion> {
        self.motions.last_mut().unwrap()
    }

    fn goto_top(&mut self) {
        self.last_motions().vertical.first();
    }

    fn goto_bottom(&mut self) {
        self.last_motions().vertical.last();
    }

    fn goto_left(&mut self) {
        self.last_motions().horizontal.first();
    }

    fn goto_right(&mut self) {
        self.last_motions().horizontal.last();
    }

    fn scroll_left(&mut self) {
        self.last_motions().horizontal.previous();
    }

    fn scroll_right(&mut self) {
        self.last_motions().horizontal.next();
    }

    fn scroll_up(&mut self) {
        self.last_motions().vertical.previous();
    }

    fn scroll_down(&mut self) {
        self.last_motions().vertical.next();
    }

    fn scroll_page_left(&mut self) {
        self.last_motions().horizontal.previous_page();
    }

    fn scroll_page_right(&mut self) {
        self.last_motions().horizontal.next_page();
    }

    fn scroll_page_up(&mut self) {
        self.last_motions().vertical.previous_page();
    }

    fn scroll_page_down(&mut self) {
        self.last_motions().vertical.next_page();
    }

    fn multiply_delay(&mut self, timer: &mut Timer, factor: u16) {
        const MAX_TIMEOUT_SECS: u64 = 24 * 3_600; // 24 hours
        let delay = timer.get_delay();
        if delay.as_secs() * (factor as u64) < MAX_TIMEOUT_SECS {
            if let Some(delay) = delay.checked_mul(factor as u32) {
                timer.set_delay(delay);
                self.every = delay;
            }
        }
    }

    fn divide_delay(&mut self, timer: &mut Timer, factor: u16) {
        const MIN_TIMEOUT_MSECS: u128 = 1;
        let delay = timer.get_delay();
        if delay.as_millis() / (factor as u128) > MIN_TIMEOUT_MSECS {
            if let Some(delay) = delay.checked_div(factor as u32) {
                timer.set_delay(delay);
                self.every = delay;
            }
        }
    }

    /// Execute an interactive action.
    fn react(&mut self, action: Action, timer: &mut Timer) -> io::Result<Action> {
        match action {
            Action::None
            | Action::ChangeScope
            | Action::SelectParent
            | Action::SelectRootPid
            | Action::SwitchBack
            | Action::SwitchToHelp
            | Action::SwitchToDetails
            | Action::SwitchToLimits
            | Action::SwitchToEnvironment
            | Action::SwitchToFiles
            | Action::SwitchToMaps
            | Action::UnselectRootPid
            | Action::Quit => (),
            Action::Filters => self.set_keymap(KeyMap::Filters),
            Action::FilterNone => {
                self.filter = ProcessFilter::None;
                self.set_keymap(KeyMap::Main);
            }
            Action::FilterUsers => {
                self.filter = ProcessFilter::UserLand;
                self.set_keymap(KeyMap::Main);
            }
            Action::FilterActive => {
                self.filter = ProcessFilter::Active;
                self.set_keymap(KeyMap::Main);
            }
            Action::FilterCurrentUser => {
                self.filter = ProcessFilter::CurrentUser;
                self.set_keymap(KeyMap::Main);
            }
            Action::MultiplyTimeout(factor) => self.multiply_delay(timer, factor),
            Action::DivideTimeout(factor) => self.divide_delay(timer, factor),
            Action::ScrollLeft => self.scroll_left(),
            Action::ScrollRight => self.scroll_right(),
            Action::ScrollPageLeft => self.scroll_page_left(),
            Action::ScrollPageRight => self.scroll_page_right(),
            Action::ScrollPageUp => self.scroll_page_up(),
            Action::ScrollPageDown => self.scroll_page_down(),
            Action::ScrollLineUp => self.scroll_up(),
            Action::ScrollLineDown => self.scroll_down(),
            Action::GotoTableTop => self.goto_top(),
            Action::GotoTableBottom => self.goto_bottom(),
            Action::GotoTableLeft => self.goto_left(),
            Action::GotoTableRight => self.goto_right(),
            Action::SearchEnter => {
                if let Some(data) = Rc::get_mut(&mut self.tree_data) {
                    data.bookmarks.incremental_search();
                    self.set_keymap(KeyMap::IncrementalSearch);
                }
            }
            Action::SearchExit => {
                self.terminal.borrow_mut().hide_cursor()?;
                if let Some(data) = Rc::get_mut(&mut self.tree_data) {
                    data.bookmarks.fixed_search();
                    self.set_keymap(KeyMap::Main);
                }
            }
            Action::SearchPush(c) => self.edit_search(SearchEdit::Push(c)),
            Action::SearchPop => self.edit_search(SearchEdit::Pop),
            Action::SearchCancel => self.clear_search(),
            Action::SelectPrevious => {
                void!(self.set_bookmarks_action(BookmarkAction::Previous))
            }
            Action::SelectNext => void!(self.set_bookmarks_action(BookmarkAction::Next)),
            Action::ClearMarks => self.clear_bookmarks(),
            Action::ToggleMarks => void!(self.set_bookmarks_action(BookmarkAction::ToggleMarks)),
        }
        Ok(action)
    }

    /// Convert the action to a possible interaction.
    fn interaction(&mut self, action: Action) -> Interaction {
        Interaction::try_from(&action).ok().unwrap_or(match action {
            Action::ChangeScope if !self.tree_data.bookmarks.marks().is_empty() => {
                let pids = self
                    .tree_data
                    .bookmarks
                    .marks()
                    .iter()
                    .copied()
                    .collect::<Vec<pid_t>>();
                self.clear_bookmarks();
                Interaction::Narrow(pids)
            }
            Action::ChangeScope => Interaction::Wide,
            Action::FilterNone
            | Action::FilterUsers
            | Action::FilterActive
            | Action::FilterCurrentUser => Interaction::Filter(self.filter),
            Action::SelectRootPid => match self.tree_data.bookmarks.selected_pid() {
                Some(selected) => Interaction::SelectRootPid(Some(*selected)),
                None => Interaction::None,
            },
            Action::UnselectRootPid => Interaction::SelectRootPid(None),
            Action::SwitchToDetails => match self.tree_data.bookmarks.selected_pid() {
                Some(selected) => Interaction::SelectPid(*selected),
                None => Interaction::None,
            },
            Action::SwitchToLimits => Interaction::SwitchTo(DataKind::Limits),
            Action::SwitchToEnvironment => Interaction::SwitchTo(DataKind::Environment),
            Action::SwitchToFiles => Interaction::SwitchTo(DataKind::Files),
            Action::SwitchToMaps => Interaction::SwitchTo(DataKind::Maps),
            _ => Interaction::None,
        })
    }

    fn search_menu<'t>(pattern: String) -> OneLineWidget<'t> {
        OneLineWidget::new(
            Text::from(format!("Search: {pattern}")),
            Style::default(),
            None,
        )
    }

    fn default_menu(&self) -> OneLineWidget<'_> {
        OneLineWidget::with_menu(self.menu.iter(), self.keymap)
    }

    /// Check if the keymap for the main pane is correct.
    fn fix_main_keymap(&self) -> Option<KeyMap> {
        let is_incremental_search = self.tree_data.bookmarks.is_incremental_search();
        match self.keymap {
            KeyMap::IncrementalSearch if is_incremental_search => None,
            KeyMap::Main if !is_incremental_search => None,
            KeyMap::Filters => None,
            _ if is_incremental_search => {
                log::warn!("{}: wrong keymap for incremental search", self.keymap);
                Some(KeyMap::IncrementalSearch)
            }
            _ => {
                log::warn!("{}: wrong keymap", self.keymap);
                Some(KeyMap::Main)
            }
        }
    }

    /// Transition between panes.
    ///
    /// Change the keymap and push or pop the position if necessary.
    fn transition(&mut self, kind: PaneKind) {
        enum Update {
            None,
            Push,
            Pop,
        }
        fn mismatch(current: PaneKind, new: PaneKind) -> (Option<KeyMap>, Update) {
            log::error!("cannot move from {:?} to {:?}", current, new);
            (None, Update::None)
        }
        let (keymap, direction) = match self.pane_kind {
            PaneKind::Main => match kind {
                PaneKind::Main => (self.fix_main_keymap(), Update::None),
                PaneKind::Help => (Some(KeyMap::Help), Update::Push),
                PaneKind::Process(DataKind::Details) => (Some(KeyMap::Details), Update::Push),
                _ => mismatch(self.pane_kind, kind),
            },
            PaneKind::Help => match kind {
                PaneKind::Help => (None, Update::None),
                PaneKind::Main => (Some(KeyMap::Main), Update::Pop),
                _ => mismatch(self.pane_kind, kind),
            },
            PaneKind::Process(DataKind::Details) => match kind {
                PaneKind::Process(DataKind::Details) => (None, Update::None),
                PaneKind::Process(DataKind::Environment | DataKind::Files | DataKind::Maps) => {
                    (Some(KeyMap::Process), Update::Push)
                }
                PaneKind::Main => (Some(KeyMap::Main), Update::Pop),
                _ => mismatch(self.pane_kind, kind),
            },
            PaneKind::Process(DataKind::Environment | DataKind::Files | DataKind::Maps) => {
                match kind {
                    PaneKind::Process(DataKind::Environment | DataKind::Files | DataKind::Maps) => {
                        (None, Update::None)
                    }
                    PaneKind::Process(DataKind::Details) => (Some(KeyMap::Details), Update::Pop),
                    _ => mismatch(self.pane_kind, kind),
                }
            }
            PaneKind::Process(DataKind::Limits) => match kind {
                PaneKind::Process(DataKind::Limits) => (None, Update::None),
                PaneKind::Process(DataKind::Details) => (Some(KeyMap::Details), Update::Pop),
                _ => mismatch(self.pane_kind, kind),
            },
            PaneKind::Process(_) => todo!("not implemented"),
        };
        if let Some(keymap) = keymap {
            self.set_keymap(keymap);
        }
        match direction {
            Update::None => (),
            Update::Push => self.motions.push(Area::default()),
            Update::Pop => void!(self.motions.pop()),
        }
        self.pane_kind = kind;
    }

    fn render_tree(&mut self, collector: &Collector) -> anyhow::Result<()> {
        let column_spacing = self.tree_data.styles.column_spacing;
        let even_row_style = self.tree_data.styles.even_row;
        let odd_row_style = self.tree_data.styles.odd_row;
        let status_style = self.tree_data.styles.status;
        let status_bar = OneLineWidget::new(Text::from(self.status_bar()), status_style, None);
        let mut motion = self.motions.pop().expect("motions for process tree");
        let selected_lineno = Rc::get_mut(&mut self.tree_data).and_then(|data| {
            data.bookmarks.selected_line(
                motion.vertical.scroll,
                &mut data.occurrences,
                collector.lines(),
            )
        });
        let (menu, show_cursor) = match self.tree_data.incremental_search_pattern() {
            Some(pattern) => (Self::search_menu(pattern), true),
            None => (self.default_menu(), false),
        };

        let mut selected_pid = None;
        {
            let table = ProcessTreeTable::new(collector, Rc::clone(&self.tree_data));
            let main = BigTableWidget::new(
                &table,
                TableStyle::new(column_spacing, even_row_style, odd_row_style),
            );
            self.terminal.borrow_mut().draw(|frame| {
                let area = frame.area();
                let mut rects = SingleScrollablePane::new(area, 3)
                    .with(&status_bar)
                    .with(&menu)
                    .build();

                let mut state = BigTableState::new(&motion, selected_lineno, 1);
                let mut cursor = if show_cursor {
                    Some(Position::new(0, area.y + area.height - 1))
                } else {
                    None
                };
                let mut r = OptionalRenderer::new(frame, &mut rects);
                r.render_stateful_widget(main, &mut state);
                r.render_widget(status_bar);
                r.render_stateful_widget(menu, &mut cursor);
                if let Some(cursor) = cursor {
                    frame.set_cursor_position(cursor);
                }
                motion = state.motion();
                selected_pid = *table.selected_pid().borrow();
            })?;
        }
        self.motions.push(motion);
        if let Some(selected_pid) = selected_pid {
            match Rc::get_mut(&mut self.tree_data) {
                Some(data) => data.bookmarks.select_pid(selected_pid),
                None => log::error!("cannot record selected PID {selected_pid}"),
            }
        }
        Ok(())
    }

    fn render_scrollable_pane<W>(&mut self, widget: W) -> anyhow::Result<()>
    where
        W: StatefulWidget<State = Zoom>,
    {
        let mut state = Zoom::with_position(self.motions.last().unwrap().vertical.position);
        let menu = self.default_menu();

        self.terminal.borrow_mut().draw(|frame| {
            let mut rects = SingleScrollablePane::new(frame.area(), 2)
                .with(&menu)
                .build();

            let mut r = OptionalRenderer::new(frame, &mut rects);
            r.render_stateful_widget(widget, &mut state);
            r.render_widget(menu);
        })?;
        if let Some(motion) = self.motions.last_mut() {
            motion.vertical.position = state.position;
        } else {
            log::error!("cannot update motion");
        }
        Ok(())
    }

    fn render_help(&mut self) -> anyhow::Result<()> {
        self.render_scrollable_pane(MarkdownWidget::new("OPRS", HELP))
    }

    fn format_option<D: fmt::Display>(option: Option<D>) -> String {
        match option {
            Some(value) => value.to_string(),
            None => "<unknown>".to_string(),
        }
    }

    fn render_details(&mut self, details: &ProcessDetails) -> anyhow::Result<()> {
        let offset = self.motions.last().unwrap().vertical.position;
        let pinfo = details.process();
        let cmdline = pinfo.cmdline();
        let metrics = details.metrics();

        let mut block_count = 0;
        let cmdline_widget =
            OneLineWidget::new(Text::from(cmdline), Style::default(), Some("Command"));
        block_count += 1;
        let cwd_widget = OneLineWidget::new(
            Text::from(process::format_result(pinfo.process().cwd())),
            Style::default(),
            Some("Working Directory"),
        );
        block_count += 1;
        let proc_fields = [
            ("Name", format!(" {} ", details.name())),
            ("Process ID", format!("{}", pinfo.pid())),
            ("Parent ID", format!("{}", pinfo.parent_pid())),
            ("Owner", TerminalDevice::format_option(pinfo.uid())),
            ("Threads", format_metric!(metrics, thread_count)),
        ];
        let proc_widget = FieldsWidget::new("Process", &proc_fields);
        let file_fields = [
            ("Descriptors", format_metric!(metrics, fd_all)),
            ("Files", format_metric!(metrics, fd_file)),
            ("I/O Read", format_metric!(metrics, io_read_total)),
            ("I/O Write", format_metric!(metrics, io_write_total)),
        ];
        let file_widget = FieldsWidget::new("Files", &file_fields);
        block_count += 1;
        let cpu_fields = [
            ("CPU", format_metric!(metrics, time_cpu)),
            ("Elapsed", format_metric!(metrics, time_elapsed)),
        ];
        let cpu_widget = FieldsWidget::new("Time", &cpu_fields);
        let mem_fields = [
            ("VM", format_metric!(metrics, mem_vm)),
            ("RSS", format_metric!(metrics, mem_rss)),
            ("Data", format_metric!(metrics, mem_data)),
        ];
        let mem_widget = FieldsWidget::new("Memory", &mem_fields);
        block_count += 1;

        let menu = self.default_menu();

        self.terminal.borrow_mut().draw(|frame| {
            let with_cmdline = offset < 1;
            let with_cwd = offset < 2;
            let with_proc_file = offset < 3;
            let mut rects = GridPane::new(frame.area())
                .with_row_if(&[&cmdline_widget], with_cmdline)
                .with_row_if(&[&cwd_widget], with_cwd)
                .with_row_if(&[&proc_widget, &file_widget], with_proc_file)
                .with_row(&[&cpu_widget, &mem_widget])
                .with_line(&menu)
                .build();
            let mut r = OptionalRenderer::new(frame, &mut rects);
            if with_cmdline {
                r.render_widget(cmdline_widget);
            }
            if with_cwd {
                r.render_widget(cwd_widget);
            }
            if with_proc_file {
                r.render_widget(proc_widget);
                r.render_widget(file_widget);
            }
            r.render_widget(cpu_widget);
            r.render_widget(mem_widget);
            r.render_widget(Clear);
            r.render_widget(menu);
        })?;
        if offset >= block_count {
            if let Some(motion) = self.motions.last_mut() {
                motion.horizontal.position = block_count.saturating_sub(1);
            }
        }
        Ok(())
    }

    fn render_table<T>(&mut self, table: T) -> anyhow::Result<()>
    where
        T: TableGenerator,
    {
        let column_spacing = self.tree_data.styles.column_spacing;
        let even_row_style = self.tree_data.styles.even_row;
        let odd_row_style = self.tree_data.styles.odd_row;
        let mut motion = self.motions.pop().expect("motions for table");
        let menu = self.default_menu();
        let main = BigTableWidget::new(
            &table,
            TableStyle::new(column_spacing, even_row_style, odd_row_style),
        );

        self.terminal.borrow_mut().draw(|frame| {
            let area = frame.area();
            let mut rects = SingleScrollablePane::new(area, 2).with(&menu).build();
            let mut r = OptionalRenderer::new(frame, &mut rects);
            let mut state = BigTableState::with_motion(&motion);
            r.render_stateful_widget(main, &mut state);
            r.render_widget(menu);
            motion = state.motion();
        })?;
        self.motions.push(motion);
        Ok(())
    }

    fn render_error<S: AsRef<str>>(&mut self, err: S) -> anyhow::Result<()> {
        let msg = OneLineWidget::new(Text::from(err.as_ref()), Style::default(), None);
        let menu = self.default_menu();

        self.terminal.borrow_mut().draw(|frame| {
            let area = frame.area();
            let mut rects = SingleScrollablePane::new(area, 2).with(&menu).build();
            let mut r = OptionalRenderer::new(frame, &mut rects);
            r.render_widget(msg);
            r.render_widget(menu);
        })?;
        Ok(())
    }

    fn render_process(&mut self, kind: DataKind, process: &Process) -> anyhow::Result<()> {
        match kind {
            DataKind::Details => panic!("implemented in render_details"),
            DataKind::Limits => match process.limits() {
                Ok(limits) => self.render_table(LimitsTable::new(limits)),
                Err(err) => self.render_error(err.to_string()),
            },
            DataKind::Environment => match process.environ() {
                Ok(env) => self.render_table(EnvironmentTable::new(env)),
                Err(err) => self.render_error(err.to_string()),
            },
            DataKind::Files => match process.fd() {
                Ok(files) => self.render_table(FilesTable::new(files)),
                Err(err) => self.render_error(err.to_string()),
            },
            DataKind::Maps => match process.maps() {
                Ok(maps) => self.render_table(MapsTable::new(maps)),
                Err(err) => self.render_error(err.to_string()),
            },
            DataKind::_Threads => self.render_error("not implemented"),
        }
    }
}

impl DisplayDevice for TerminalDevice {
    fn open(&mut self, metrics: SliceIter<FormattedMetric>) -> anyhow::Result<()> {
        let mut last_id = None;

        Collector::for_each_computed_metric(metrics, |id, ag| {
            let mut header = id
                .as_str()
                .split(":")
                .map(str::to_string)
                .collect::<Vec<String>>();
            if last_id.is_none() || last_id.unwrap() != id {
                last_id = Some(id);
            } else {
                let name = format!(
                    "{} ({})",
                    header.pop().unwrap(),
                    match ag {
                        Aggregation::None => "none", // never used
                        Aggregation::Min => "min",
                        Aggregation::Max => "max",
                        Aggregation::Ratio => "%",
                    }
                );
                header.push(name);
            }
            if let Some(data) = Rc::get_mut(&mut self.tree_data) {
                data.metric_headers.push(header.join("\n"));
            }
        });
        self.terminal.borrow_mut().hide_cursor()?;
        Ok(())
    }

    /// Show the cursor on exit.
    fn close(&mut self) -> anyhow::Result<()> {
        self.terminal.borrow_mut().show_cursor()?;
        Ok(())
    }

    /// Render the current pane.
    fn render(&mut self, kind: PaneKind, data: PaneData, _redraw: bool) -> anyhow::Result<()> {
        self.transition(kind);
        match (kind, data) {
            (PaneKind::Main, PaneData::Collector(collector)) => self.render_tree(collector),
            (PaneKind::Process(DataKind::Details), PaneData::Details(details)) => {
                self.render_details(details)
            }
            (PaneKind::Process(kind), PaneData::Process(proc)) => self.render_process(kind, proc),
            (PaneKind::Help, _) => self.render_help(),

            (kind, _) => panic!("{kind:?}: invalid pane kind or data"),
        }
    }

    /// Wait for a user input or a timeout.
    fn pause(&mut self, timer: &mut Timer) -> anyhow::Result<PauseStatus> {
        if let Some(timeout) = timer.remaining() {
            if let Some(evt) = self.events.receive_timeout(timeout)? {
                let action = self.react(self.keymap.action_from_event(evt), timer)?;
                Ok(PauseStatus::Action(self.interaction(action)))
            } else {
                Ok(PauseStatus::TimeOut)
            }
        } else {
            Ok(PauseStatus::TimeOut)
        }
    }
}
