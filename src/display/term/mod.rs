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
use itertools::izip;
use libc::pid_t;
use ratatui::{
    backend::TermionBackend,
    layout::Alignment,
    prelude::*,
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Cell, Clear},
    Terminal,
};
use std::{
    cmp::Ordering, collections::BTreeSet, convert::TryFrom, fmt, io, rc::Rc, time::Duration,
};
use termion::{
    raw::{IntoRawMode, RawTerminal},
    screen::{AlternateScreen, IntoAlternateScreen},
};

use crate::{
    clock::Timer,
    console::{is_tty, BuiltinTheme, EventChannel},
    process::{
        self, format::human_duration, Aggregation, Collector, FormattedMetric, LimitKind, Process,
        ProcessDetails, ProcessFilter, ProcessIdentity, ProcessSamples,
    },
};

use super::{DataKind, DisplayDevice, PaneData, PaneKind, PauseStatus, SliceIter};

mod input;
mod panes;

#[macro_use]
mod types;

use input::{menu, Action, BookmarkAction, Bookmarks, KeyMap, MenuEntry, SearchEdit};
use panes::{
    BigTableWidget, DoubleZoom, FieldsWidget, GridPane, MarkdownWidget, OneLineWidget,
    OptionalRenderer, Pane, SingleScrollablePane, TableGenerator, TableStyle, Zoom,
};
use types::{Area, MaxLength, UnboundedArea};

/// Right aligned cell.
macro_rules! rcell {
    ($s:expr) => {
        Cell::from(Text::from($s).alignment(Alignment::Right))
    };
}

const HELP: &str = include_str!("help_en.md");

/// User action that has an impact on the application.
#[derive(Clone, Debug)]
pub enum Interaction {
    None,
    Filter(ProcessFilter),
    SwitchToHelp,
    SwitchBack,
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

/// Status of a process.
#[derive(Clone, Copy, Debug)]
pub enum PidStatus {
    /// No specific status.
    Unknown,
    /// Under the cursor.
    Selected,
    /// Bookmarked.
    Marked,
    /// Search match.
    Matching,
}

/// Theme styles
#[derive(Debug)]
struct Styles {
    /// Even rows
    even_row: Style,
    /// Odd rows
    odd_row: Style,
    /// Increasing value
    increase: Style,
    /// Decreasing value
    decrease: Style,
    /// Unselected line
    unselected: Style,
    /// Selected line
    selected: Style,
    /// Bookmarked line.
    marked: Style,
    /// Matching line
    matching: Style,
    /// Status line
    status: Style,
    /// Space between columns in number of characters
    column_spacing: u16,
}

impl Styles {
    fn new(theme: Option<BuiltinTheme>) -> Self {
        let default_style = Style::default();
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let bold_reversed = bold.add_modifier(Modifier::REVERSED);
        let white_on_blue = Style::default().fg(Color::White).bg(Color::Blue);
        match theme {
            Some(BuiltinTheme::Dark) => Styles {
                even_row: default_style,
                odd_row: Style::default().bg(Color::Indexed(238)),
                increase: Style::default().fg(Color::Indexed(196)),
                decrease: Style::default().fg(Color::Indexed(46)),
                unselected: bold,
                selected: Style::default().fg(Color::Black).bg(Color::LightMagenta),
                marked: Style::default().fg(Color::LightCyan),
                matching: Style::default().fg(Color::LightMagenta),
                status: white_on_blue,
                column_spacing: 2,
            },
            Some(BuiltinTheme::Light) => Styles {
                even_row: default_style,
                odd_row: Style::default().bg(Color::Indexed(254)),
                increase: Style::default().fg(Color::Indexed(124)),
                decrease: Style::default().fg(Color::Indexed(40)),
                unselected: bold,
                selected: Style::default().fg(Color::White).bg(Color::Magenta),
                marked: Style::default().fg(Color::Cyan),
                matching: Style::default().fg(Color::Magenta),
                status: white_on_blue,
                column_spacing: 2,
            },
            Some(BuiltinTheme::Dark16) => Styles {
                even_row: default_style,
                odd_row: default_style,
                increase: Style::default().fg(Color::LightMagenta),
                decrease: Style::default().fg(Color::LightGreen),
                unselected: bold,
                selected: Style::default().fg(Color::Black).bg(Color::LightMagenta),
                marked: Style::default().fg(Color::LightCyan),
                matching: Style::default().fg(Color::LightMagenta),
                status: white_on_blue,
                column_spacing: 2,
            },
            Some(BuiltinTheme::Light16) => Styles {
                even_row: default_style,
                odd_row: default_style,
                increase: Style::default().fg(Color::Red),
                decrease: Style::default().fg(Color::Green),
                unselected: bold,
                selected: Style::default().fg(Color::White).bg(Color::Magenta),
                marked: Style::default().fg(Color::Cyan),
                matching: Style::default().fg(Color::Magenta),
                status: white_on_blue,
                column_spacing: 2,
            },
            None => Styles {
                even_row: default_style,
                odd_row: default_style,
                increase: bold,
                decrease: bold,
                unselected: bold,
                selected: bold_reversed,
                marked: bold.add_modifier(Modifier::UNDERLINED),
                matching: Style::default().add_modifier(Modifier::UNDERLINED),
                status: bold_reversed,
                column_spacing: 2,
            },
        }
    }

    fn name_style(&self, status: PidStatus) -> Style {
        match status {
            PidStatus::Unknown => self.unselected,
            PidStatus::Selected => self.selected,
            PidStatus::Marked => self.marked,
            PidStatus::Matching => self.matching,
        }
    }

    fn trend_style(&self, trend: &Ordering) -> Style {
        match trend {
            Ordering::Less => self.decrease,
            Ordering::Equal => Style::default(),
            Ordering::Greater => self.increase,
        }
    }
}

/// Stack of parent child PIDs
#[derive(Default)]
struct PidStack(Vec<pid_t>);

impl PidStack {
    /// Stack len
    fn len(&self) -> usize {
        self.0.len()
    }

    /// Pop pids that are not a parent of the given process and push the new pid on the stack.
    fn push(&mut self, samples: &ProcessSamples) {
        let Self(ref mut stack) = self;
        match samples.parent_pid() {
            Some(parent_pid) => {
                while let Some(top_pid) = stack.last() {
                    if *top_pid == parent_pid {
                        break;
                    }
                    let _ = stack.pop();
                }
                stack.push(samples.pid());
            }
            None => stack.clear(), // Cannot happened. Only the system has no parent.
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum VerticalScroll {
    Line(usize),
    Block,
}

impl Into<u16> for VerticalScroll {
    // From<u16> cannot be implemented since there is no way to tell if it's a
    // line or a block.
    #![allow(clippy::from_over_into)]
    fn into(self) -> u16 {
        match self {
            Self::Line(value) => value as u16,
            Self::Block => 1u16,
        }
    }
}

macro_rules! format_metric {
    ($metrics:expr, $field:ident) => {
        TerminalDevice::format_option($metrics.as_ref().and_then(|m| m.$field.strings().next()))
    };
}

/// Data used to generate the tree as a table.
#[derive(Debug)]
struct TreeData<'t> {
    /// Column headers for metrics
    metric_headers: Vec<Text<'t>>,
    /// Display styles
    styles: Styles,
    /// Bookmarks for PIDs.
    bookmarks: Bookmarks,
    /// PID matched by a search.
    occurrences: BTreeSet<pid_t>,
}

impl<'t> TreeData<'t> {
    fn new(styles: Styles) -> Self {
        Self {
            metric_headers: Vec::new(),
            styles,
            bookmarks: Bookmarks::default(),
            occurrences: BTreeSet::default(),
        }
    }

    /// Status of a process.
    fn pid_status(&self, pid: pid_t) -> PidStatus {
        if self.bookmarks.is_selected(pid) {
            PidStatus::Selected
        } else if self.occurrences.contains(&pid) {
            PidStatus::Matching
        } else if self.bookmarks.is_marked(pid) {
            PidStatus::Marked
        } else {
            PidStatus::Unknown
        }
    }
}

/// Table generator for a tree of processes.
struct ProcessTreeTable<'a, 'b, 't> {
    /// Sample collector.
    collector: &'b Collector<'a>,
    /// Tree data.
    data: Rc<TreeData<'t>>,
    /// Headers size.
    headers_size: Area<usize>,
    /// Column widths
    widths: Vec<u16>,
    /// Indentation
    indents: Vec<usize>,
}

impl<'a, 'b, 't> ProcessTreeTable<'a, 'b, 't> {
    const TITLE_PROCESS: &'static str = "Process";
    const TITLE_PID: &'static str = "PID";
    const TITLE_STATE: &'static str = "S";
    const FIXED_HEADERS: [&'static str; 3] =
        [Self::TITLE_PROCESS, Self::TITLE_PID, Self::TITLE_STATE];

    fn new(collector: &'b Collector<'a>, data: Rc<TreeData<'t>>) -> Self {
        let mut pids = PidStack::default();
        let mut headers_height = 0;
        let mut widths = Self::FIXED_HEADERS
            .iter()
            .map(|s| MaxLength::from(*s))
            .chain(data.metric_headers.iter().map(|text| {
                if headers_height < text.lines.len() {
                    headers_height = text.lines.len();
                }
                MaxLength::from(text.iter().map(|line| line.width()).max().unwrap_or(0))
            }))
            .collect::<Vec<MaxLength>>();
        let headers_size = Area::new(Self::FIXED_HEADERS.len(), headers_height);
        let mut indents = Vec::with_capacity(collector.line_count());
        collector.lines().for_each(|ps| {
            pids.push(ps);
            let indent = pids.len().saturating_sub(1);
            indents.push(indent);
            widths[0].set_min(indent + ps.name().len());
            widths[1].set_min(ps.pid().to_string().len());
            // widths[2].set_min(1);
            ps.samples().enumerate().for_each(|(i, s)| {
                widths[i + headers_size.horizontal]
                    .set_min(s.strings().map(|s| s.len()).max().unwrap_or(0))
            });
        });
        Self {
            collector,
            headers_size,
            data,
            widths: widths.iter().map(|ml| ml.len()).collect::<Vec<u16>>(),
            indents,
        }
    }

    /// Number of columns in the body.
    fn body_column_count(&self) -> usize {
        self.data.metric_headers.len()
    }

    /// Number of rows in the body.
    fn body_row_count(&self) -> usize {
        self.collector.line_count()
    }
}

impl<'a, 'b, 't> TableGenerator for ProcessTreeTable<'a, 'b, 't> {
    fn headers_size(&self) -> Area<usize> {
        self.headers_size
    }

    fn top_headers(&self, zoom: &Zoom) -> Vec<Cell> {
        Self::FIXED_HEADERS
            .iter()
            .map(|s| Cell::from(Text::from(*s)))
            .chain(
                self.data
                    .metric_headers
                    .iter()
                    .skip(zoom.position)
                    .take(zoom.visible_length)
                    .map(|text| Cell::from(text.clone().alignment(Alignment::Center))),
            )
            .collect::<Vec<Cell>>()
    }

    fn rows(&self, zoom: &DoubleZoom) -> Vec<Vec<Cell>> {
        self.collector
            .lines()
            .skip(zoom.vertical.position)
            .take(zoom.vertical.visible_length)
            .enumerate()
            .map(|(n, ps)| {
                let pid_status = self.data.pid_status(ps.pid());
                let name = {
                    let name = ps.name();
                    format!("{:>width$}", name, width = self.indents[n] + name.len())
                };
                let name_style = self.data.styles.name_style(pid_status);
                vec![
                    Cell::from(name).style(name_style),
                    rcell!(ps.pid().to_string()),
                    rcell!(ps.state().to_string()),
                ]
                .drain(..)
                .chain(
                    ps.samples()
                        .flat_map(|sample| {
                            izip!(sample.strings(), sample.trends()).map(|(value, trend)| {
                                Cell::from(
                                    Text::from(value.as_str())
                                        .style(self.data.styles.trend_style(trend))
                                        .alignment(Alignment::Right),
                                )
                            })
                        })
                        .skip(zoom.horizontal.position)
                        .take(zoom.horizontal.visible_length),
                )
                .collect::<Vec<Cell>>()
            })
            .collect::<Vec<Vec<Cell>>>()
    }

    fn widths(&self) -> &[u16] {
        &self.widths
    }
}

/// Print on standard output as a table
pub struct TerminalDevice<'t> {
    /// Interval to update the screen
    every: Duration,
    /// Channel for input events
    events: EventChannel,
    /// Terminal
    terminal: Terminal<TermionBackend<Box<AlternateScreen<RawTerminal<io::Stdout>>>>>,
    /// Table tree data
    tree_data: Rc<TreeData<'t>>,
    /// Horizontal and vertical offset
    table_offset: UnboundedArea,
    /// Pane offset (except for the table)
    pane_offset: u16,
    /// Number of lines to scroll vertically up and down
    vertical_scroll: VerticalScroll,
    /// Horizontal and vertical overflow (whether the table is bigger than the screen)
    overflow: Area<bool>,
    /// Slots where limits are displayed under the metric (only for raw metrics).
    limit_slots: Vec<bool>,
    /// Mode to display limits.
    display_limits: LimitKind,
    /// Number of available lines to display the table
    body_height: usize,
    /// Filter
    filter: ProcessFilter,
    /// Menu
    menu: Vec<MenuEntry>,
    /// Pane kind.
    pane_kind: PaneKind,
    /// Key map
    keymap: KeyMap,
}

impl TerminalDevice<'_> {
    pub fn new(every: Duration, theme: Option<BuiltinTheme>) -> anyhow::Result<Self> {
        let screen = io::stdout().into_raw_mode()?.into_alternate_screen()?;
        let backend = TermionBackend::new(Box::new(screen));
        let terminal = Terminal::new(backend)?;

        Ok(TerminalDevice {
            every,
            events: EventChannel::new(),
            terminal,
            tree_data: Rc::new(TreeData::new(Styles::new(theme))),
            table_offset: Default::default(),
            pane_offset: 0,
            vertical_scroll: VerticalScroll::Line(1),
            overflow: Area::default(),
            limit_slots: Vec::new(),
            display_limits: LimitKind::None,
            body_height: 0,
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
                "{time_string} -- interval:{delay} -- limit:{} -- filter:{}",
                self.display_limits.as_ref(),
                self.filter
            )
        }
    }

    /// Clear marks.
    fn clear_bookmarks(&mut self) {
        void!(Rc::get_mut(&mut self.tree_data).map(|data| data.bookmarks.clear_marks()))
    }

    /// Clear search.
    fn clear_search(&mut self) {
        void!(Rc::get_mut(&mut self.tree_data).map(|data| data.bookmarks.clear_search()))
    }

    /// Edit search.
    fn edit_search(&mut self, edit: SearchEdit) {
        Rc::get_mut(&mut self.tree_data).map(|data| data.bookmarks.edit_search(edit));
    }

    /// Set bookmark action.
    fn set_bookmarks_action(&mut self, action: BookmarkAction) {
        void!(Rc::get_mut(&mut self.tree_data).map(|data| data.bookmarks.set_action(action)))
    }

    /// Clear bookmarks and set bookmark action if the condition is true.
    fn clear_and_set_bookmarks_action_if(&mut self, action: BookmarkAction, cond: bool) {
        Rc::get_mut(&mut self.tree_data).map(|data| {
            data.bookmarks.clear_marks();
            if cond {
                void!(data.bookmarks.set_action(action));
            }
        });
    }

    /// Clear bookmarks and set bookmark action.
    fn clear_and_set_bookmarks_action(&mut self, action: BookmarkAction) {
        self.clear_and_set_bookmarks_action_if(action, true);
    }

    /// Execute an interactive action.
    fn react(&mut self, action: Action, timer: &mut Timer) -> io::Result<Action> {
        const MAX_TIMEOUT_SECS: u64 = 24 * 3_600; // 24 hours
        const MIN_TIMEOUT_MSECS: u128 = 1;
        match action {
            Action::None
            | Action::ChangeScope
            | Action::SelectParent
            | Action::SelectRootPid
            | Action::SwitchToHelp
            | Action::SwitchToProcess
            | Action::UnselectRootPid
            | Action::Quit => (),
            Action::SwitchBack => {
                self.set_keymap(KeyMap::Main);
                self.pane_offset = 0;
            }
            Action::Filters => self.set_keymap(KeyMap::Filters),
            Action::FilterNone => {
                self.filter = ProcessFilter::None;
                self.set_keymap(KeyMap::Main);
            }
            Action::FilterUser => {
                self.filter = ProcessFilter::UserLand;
                self.set_keymap(KeyMap::Main);
            }
            Action::FilterActive => {
                self.filter = ProcessFilter::Active;
                self.set_keymap(KeyMap::Main);
            }
            Action::MultiplyTimeout(factor) => {
                let delay = timer.get_delay();
                if delay.as_secs() * (factor as u64) < MAX_TIMEOUT_SECS {
                    if let Some(delay) = delay.checked_mul(factor as u32) {
                        timer.set_delay(delay);
                        self.every = delay;
                    }
                }
            }
            Action::DivideTimeout(factor) => {
                let delay = timer.get_delay();
                if delay.as_millis() / (factor as u128) > MIN_TIMEOUT_MSECS {
                    if let Some(delay) = delay.checked_div(factor as u32) {
                        timer.set_delay(delay);
                        self.every = delay;
                    }
                }
            }
            Action::ToggleLimits => {
                self.display_limits = match self.display_limits {
                    LimitKind::None => LimitKind::Soft,
                    LimitKind::Soft => LimitKind::Hard,
                    LimitKind::Hard => LimitKind::None,
                }
            }
            Action::ScrollLeft => self.table_offset.scroll_left(1),
            Action::ScrollRight => {
                if self.overflow.horizontal {
                    self.table_offset.scroll_right(1);
                }
            }
            Action::ScrollPageUp => match self.pane_kind {
                PaneKind::Main => self.clear_and_set_bookmarks_action(BookmarkAction::PreviousPage),
                _ => {
                    self.pane_offset = self.pane_offset.saturating_sub(self.vertical_scroll.into());
                }
            },
            Action::ScrollPageDown => match self.pane_kind {
                PaneKind::Main => self.clear_and_set_bookmarks_action_if(
                    BookmarkAction::NextPage,
                    self.overflow.vertical,
                ),
                _ => {
                    self.pane_offset = self.pane_offset.saturating_add(self.vertical_scroll.into());
                }
            },
            Action::ScrollLineUp => {
                self.clear_and_set_bookmarks_action(BookmarkAction::PreviousLine)
            }
            Action::ScrollLineDown => self.clear_and_set_bookmarks_action(BookmarkAction::NextLine),
            Action::GotoTableTop => void!(self.set_bookmarks_action(BookmarkAction::FirstLine)),
            Action::GotoTableBottom => void!(self.set_bookmarks_action(BookmarkAction::LastLine)),
            Action::GotoTableLeft => self.table_offset.horizontal_home(),
            Action::GotoTableRight => self.table_offset.horizontal_end(),
            Action::SearchEnter => {
                self.set_keymap(KeyMap::IncrementalSearch);
                Rc::get_mut(&mut self.tree_data).map(|data| data.bookmarks.incremental_search());
            }
            Action::SearchExit => {
                self.terminal.hide_cursor()?;
                self.set_keymap(KeyMap::Main);
                Rc::get_mut(&mut self.tree_data).map(|data| data.bookmarks.fixed_search());
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
            Action::FilterNone | Action::FilterUser | Action::FilterActive => {
                Interaction::Filter(self.filter)
            }
            Action::SelectRootPid => match self.tree_data.bookmarks.selected() {
                Some(selected) => Interaction::SelectRootPid(Some(selected.pid)),
                None => Interaction::None,
            },
            Action::UnselectRootPid => Interaction::SelectRootPid(None),
            Action::SwitchToProcess => match self.tree_data.bookmarks.selected() {
                Some(selected) => Interaction::SelectPid(selected.pid),
                None => Interaction::None,
            },
            _ => Interaction::None,
        })
    }

    fn top(&self, line_count: usize) -> usize {
        let top = self
            .table_offset
            .vertical
            .value()
            .copied()
            .unwrap_or(line_count);
        if top >= line_count {
            line_count.saturating_sub(self.body_height)
        } else {
            top
        }
    }

    fn render_tree(&mut self, collector: &Collector) -> anyhow::Result<()> {
        self.pane_kind = PaneKind::Main;

        let metric_headers_len = self.tree_data.metric_headers.len();
        let line_count = collector.line_count();
        let top = self.top(line_count);
        let voffset = Rc::get_mut(&mut self.tree_data)
            .map(|data| {
                data.bookmarks.execute(
                    &mut data.occurrences,
                    collector.lines(),
                    top,
                    self.body_height,
                )
            })
            .unwrap_or(0);
        self.table_offset.set_vertical(voffset);
        self.table_offset.set_bounds(
            metric_headers_len.saturating_sub(1),
            line_count.saturating_sub(self.body_height),
        );
        let column_spacing = self.tree_data.styles.column_spacing;
        let even_row_style = self.tree_data.styles.even_row;
        let odd_row_style = self.tree_data.styles.odd_row;
        let status_style = self.tree_data.styles.status;
        let is_search = self.tree_data.bookmarks.is_incremental_search();
        let mut body_height = 0;
        let show_cursor = is_search;
        let status_bar = OneLineWidget::new(Text::from(self.status_bar()), status_style, None);
        let menu = if is_search {
            OneLineWidget::new(
                Text::from(format!(
                    "Search: {}",
                    self.tree_data.bookmarks.search_pattern().unwrap()
                )),
                Style::default(),
                None,
            )
        } else {
            OneLineWidget::with_menu(self.menu.iter(), self.keymap)
        };

        let table = ProcessTreeTable::new(collector, Rc::clone(&self.tree_data));
        let main = BigTableWidget::new(
            &table,
            TableStyle::new(column_spacing, even_row_style, odd_row_style),
        );

        let mut new_overflow = Area::default();
        self.terminal.draw(|frame| {
            let area = frame.area();
            let mut rects = SingleScrollablePane::new(area, 3)
                .with(&status_bar)
                .with(&menu)
                .build();

            let mut state = DoubleZoom::new(
                Zoom::new(
                    self.table_offset.horizontal.value_or_zero(),
                    0,
                    table.body_column_count(),
                ),
                Zoom::new(
                    self.table_offset.vertical.value_or_zero(),
                    0,
                    table.body_row_count(),
                ),
            );
            let mut cursor = if show_cursor {
                Some(Position::new(0, area.y + area.height - 1))
            } else {
                None
            };
            let mut r = OptionalRenderer::new(frame, &mut rects);
            r.render_stateful_widget(main, &mut state);
            r.render_widget(status_bar);
            r.render_stateful_widget(menu, &mut cursor);
            body_height = state.vertical.visible_length - table.headers_size.vertical;
            new_overflow = Area::new(!state.horizontal.at_end(), !state.vertical.at_end());
            log::debug!(
                "state: {:?} -- overflow: {:?}",
                state.horizontal,
                new_overflow.horizontal
            );
            if let Some(cursor) = cursor {
                frame.set_cursor_position(cursor);
            }
        })?;
        self.overflow = new_overflow;
        self.vertical_scroll = VerticalScroll::Line(body_height.div_ceil(2) as usize);
        self.body_height = body_height as usize;
        Ok(())
    }

    fn render_scrollable_pane<W>(&mut self, widget: W) -> anyhow::Result<()>
    where
        W: StatefulWidget<State = Zoom>,
    {
        let mut state = Zoom::with_position(self.pane_offset as usize);
        let menu = OneLineWidget::with_menu(self.menu.iter(), self.keymap);

        self.terminal.draw(|frame| {
            let mut rects = SingleScrollablePane::new(frame.area(), 2)
                .with(&menu)
                .build();

            let mut r = OptionalRenderer::new(frame, &mut rects);
            r.render_stateful_widget(widget, &mut state);
            r.render_widget(menu);
            self.pane_offset = state.position as u16;
            self.vertical_scroll = VerticalScroll::Line(state.visible_length.div_ceil(2) as usize);
        })?;
        Ok(())
    }

    fn render_help(&mut self) -> anyhow::Result<()> {
        self.pane_kind = PaneKind::Help;
        self.render_scrollable_pane(MarkdownWidget::new("OPRS", HELP))
    }

    fn format_option<D: fmt::Display>(option: Option<D>) -> String {
        match option {
            Some(value) => value.to_string(),
            None => "<unknown>".to_string(),
        }
    }

    fn render_details(&mut self, details: &ProcessDetails) -> anyhow::Result<()> {
        self.pane_kind = PaneKind::Process(DataKind::Details);
        let offset = self.pane_offset;
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

        let menu = OneLineWidget::with_menu(self.menu.iter(), self.keymap);

        self.terminal.draw(|frame| {
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
        if self.pane_offset >= block_count {
            self.pane_offset = block_count.saturating_sub(1);
        }
        self.vertical_scroll = VerticalScroll::Block; // scrolling by block not by line.
        Ok(())
    }

    fn render_process(&mut self, kind: DataKind, _process: &Process) -> anyhow::Result<()> {
        self.pane_kind = PaneKind::Process(kind);
        panic!("not implemented");
    }
}

impl DisplayDevice for TerminalDevice<'_> {
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
                self.limit_slots.push(true);
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
                self.limit_slots.push(false);
            }
            Rc::get_mut(&mut self.tree_data).map(|data| {
                data.metric_headers.push(Text::from(
                    header
                        .iter()
                        .map(|s| Line::from(s.to_string()))
                        .collect::<Vec<Line>>(),
                ))
            });
        });
        self.terminal.hide_cursor()?;
        Ok(())
    }

    /// Show the cursor on exit.
    fn close(&mut self) -> anyhow::Result<()> {
        self.terminal.show_cursor()?;
        Ok(())
    }

    /// Render the current pane.
    fn render(&mut self, kind: PaneKind, data: PaneData, _redraw: bool) -> anyhow::Result<()> {
        match (kind, data) {
            (PaneKind::Main, PaneData::Collector(collector)) => {
                let is_incremental_search = self.tree_data.bookmarks.is_incremental_search();
                match self.keymap {
                    KeyMap::IncrementalSearch if is_incremental_search => (),
                    KeyMap::Main if !is_incremental_search => (),
                    KeyMap::Filters => (),
                    _ if is_incremental_search => {
                        log::error!("{}: wrong keymap for incremental search", self.keymap);
                        self.set_keymap(KeyMap::IncrementalSearch);
                    }
                    _ => {
                        log::error!("{}: wrong keymap", self.keymap);
                        self.set_keymap(KeyMap::Main);
                    }
                }
                self.render_tree(collector)
            }
            (PaneKind::Process(DataKind::Details), PaneData::Details(details)) => {
                self.set_keymap(KeyMap::Details);
                self.render_details(details)
            }
            (PaneKind::Process(kind), PaneData::Process(proc)) => {
                self.set_keymap(KeyMap::Process);
                self.render_process(kind, proc)
            }
            (PaneKind::Help, _) => {
                self.set_keymap(KeyMap::Help);
                self.render_help()
            }
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
