// Oprs -- process monitor for Linux
// Copyright (C) 2020-2024  Laurent Pelecq
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
use std::{cmp::Ordering, collections::BTreeSet, convert::TryFrom, fmt, io, time::Duration};
use termion::{
    raw::{IntoRawMode, RawTerminal},
    screen::{AlternateScreen, IntoAlternateScreen},
};

use crate::{
    clock::Timer,
    console::{is_tty, BuiltinTheme, EventChannel},
    process::{
        format::human_duration, Aggregation, Collector, FormattedMetric, LimitKind, ProcessDetails,
        ProcessFilter, ProcessIdentity, ProcessSamples,
    },
};

use super::{DisplayDevice, PaneData, PaneKind, PauseStatus, SliceIter};

mod input;
mod panes;

#[macro_use]
mod types;

use input::{menu, Action, BookmarkAction, Bookmarks, KeyMap, MenuEntry, SearchEdit};
use panes::{
    BigTableWidget, FieldsWidget, GridPane, MarkdownWidget, OneLineWidget, OptionalRenderer, Pane,
    ReactiveWidget, SingleScrollablePane, TableStyle,
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

macro_rules! format_metric {
    ($metrics:expr, $field:ident) => {
        TerminalDevice::format_option($metrics.as_ref().and_then(|m| m.$field.strings().next()))
    };
}

/// Print on standard output as a table
pub struct TerminalDevice<'t> {
    /// Interval to update the screen
    every: Duration,
    /// Channel for input events
    events: EventChannel,
    /// Terminal
    terminal: Terminal<TermionBackend<Box<AlternateScreen<RawTerminal<io::Stdout>>>>>,
    /// Horizontal and vertical offset
    table_offset: UnboundedArea,
    /// Pane offset (except for the table)
    pane_offset: u16,
    /// Number of lines to scroll vertically up and down
    vertical_scroll: usize,
    /// Horizontal and vertical overflow (whether the table is bigger than the screen)
    overflow: Area<bool>,
    /// Column headers for metrics
    metric_headers: Vec<Text<'t>>,
    /// Slots where limits are displayed under the metric (only for raw metrics).
    limit_slots: Vec<bool>,
    /// Mode to display limits.
    display_limits: LimitKind,
    /// Display styles
    styles: Styles,
    /// Number of lines in the headers
    headers_height: usize,
    /// Number of available lines to display the table
    body_height: usize,
    /// Bookmarks for PIDs.
    bookmarks: Bookmarks,
    /// PID matched by a search.
    occurrences: BTreeSet<pid_t>,
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
            table_offset: Default::default(),
            pane_offset: 0,
            vertical_scroll: 1,
            overflow: Area::default(),
            metric_headers: Vec::new(),
            limit_slots: Vec::new(),
            display_limits: LimitKind::None,
            styles: Styles::new(theme),
            headers_height: 0,
            body_height: 0,
            bookmarks: Bookmarks::default(),
            occurrences: BTreeSet::default(),
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
        let matches_count = self.occurrences.len();
        let marks_count = self.bookmarks.marks().len();
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

    /// Draw the table of metrics and the menu.
    ///
    /// Return the table visible height.
    fn draw_tree(
        &mut self,
        headers: Vec<Cell>,
        rows: Vec<Vec<Cell>>,
        col_widths: &[u16],
    ) -> anyhow::Result<()> {
        let column_spacing = self.styles.column_spacing;
        let even_row_style = self.styles.even_row;
        let odd_row_style = self.styles.odd_row;
        let mut body_height = 0;
        let headers_height = self.headers_height as u16;
        let is_search = self.bookmarks.is_incremental_search();
        let show_cursor = is_search;
        let mut main = BigTableWidget::new(
            headers,
            headers_height,
            rows,
            col_widths,
            self.table_offset,
            TableStyle::new(column_spacing, even_row_style, odd_row_style),
        );
        let status_bar =
            OneLineWidget::new(Text::from(self.status_bar()), self.styles.status, None);
        let menu = if is_search {
            OneLineWidget::new(
                Text::from(format!(
                    "Search: {}",
                    self.bookmarks.search_pattern().unwrap()
                )),
                Style::default(),
                None,
            )
        } else {
            OneLineWidget::with_menu(self.menu.iter(), self.keymap)
        };

        let mut new_overflow = Area::default();
        self.terminal.draw(|frame| {
            let area = frame.area();
            let mut rects = SingleScrollablePane::new(area, 3)
                .with(&status_bar)
                .with(&menu)
                .build();

            if let Some(Some(main_rect)) = rects.first_mut() {
                let (inner_height, overflow) = main.prepare(main_rect);
                body_height = inner_height - headers_height;
                new_overflow = overflow;
            }
            let cursor = if show_cursor {
                menu.cursor()
                    .map(|p| Position::new(p.x, area.y + area.height - 1))
            } else {
                None
            };
            let mut r = OptionalRenderer::new(frame, &mut rects);
            r.render_widget(main);
            r.render_widget(status_bar);
            r.render_widget(menu);
            if let Some(cursor) = cursor {
                frame.set_cursor_position(cursor);
            }
        })?;
        self.overflow = new_overflow;
        self.vertical_scroll = body_height.div_ceil(2) as usize;
        self.body_height = body_height as usize;
        Ok(())
    }

    /// Execute an interactive action.
    fn react(&mut self, action: Action, timer: &mut Timer) -> io::Result<Action> {
        const MAX_TIMEOUT_SECS: u64 = 24 * 3_600; // 24 hours
        const MIN_TIMEOUT_MSECS: u128 = 1;
        match action {
            Action::None
            | Action::SelectParent
            | Action::Quit
            | Action::SwitchToHelp
            | Action::SwitchToProcess
            | Action::ChangeScope => (),
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
                PaneKind::Main => {
                    self.bookmarks.clear_search();
                    void!(self.bookmarks.set_action(BookmarkAction::PreviousPage));
                }
                _ => {
                    self.pane_offset = self.pane_offset.saturating_sub(self.vertical_scroll as u16);
                }
            },
            Action::ScrollPageDown => match self.pane_kind {
                PaneKind::Main => {
                    self.bookmarks.clear_search();
                    if self.overflow.vertical {
                        void!(self.bookmarks.set_action(BookmarkAction::NextPage))
                    }
                }
                _ => {
                    self.pane_offset += self.vertical_scroll as u16;
                }
            },
            Action::ScrollLineUp => {
                self.bookmarks.clear_search();
                void!(self.bookmarks.set_action(BookmarkAction::PreviousLine));
            }
            Action::ScrollLineDown => {
                self.bookmarks.clear_search();
                void!(self.bookmarks.set_action(BookmarkAction::NextLine))
            }
            Action::GotoTableTop => void!(self.bookmarks.set_action(BookmarkAction::FirstLine)),
            Action::GotoTableBottom => void!(self.bookmarks.set_action(BookmarkAction::LastLine)),
            Action::GotoTableLeft => self.table_offset.horizontal_home(),
            Action::GotoTableRight => self.table_offset.horizontal_end(),
            Action::SearchEnter => {
                self.set_keymap(KeyMap::IncrementalSearch);
                self.bookmarks.incremental_search();
            }
            Action::SearchExit => {
                self.terminal.hide_cursor()?;
                self.set_keymap(KeyMap::Main);
                self.bookmarks.fixed_search()
            }
            Action::SearchPush(c) => self.bookmarks.edit_search(SearchEdit::Push(c)),
            Action::SearchPop => self.bookmarks.edit_search(SearchEdit::Pop),
            Action::SearchCancel => self.bookmarks.clear_search(),
            Action::SelectPrevious => {
                void!(self.bookmarks.set_action(BookmarkAction::Previous))
            }
            Action::SelectNext => void!(self.bookmarks.set_action(BookmarkAction::Next)),
            Action::ClearMarks => self.bookmarks.clear_marks(),
            Action::ToggleMarks => void!(self.bookmarks.set_action(BookmarkAction::ToggleMarks)),
        }
        Ok(action)
    }

    /// Convert the action to a possible interaction.
    fn interaction(&mut self, action: Action) -> Interaction {
        Interaction::try_from(&action).ok().unwrap_or(match action {
            Action::ChangeScope if !self.bookmarks.marks().is_empty() => {
                let pids = self
                    .bookmarks
                    .marks()
                    .iter()
                    .copied()
                    .collect::<Vec<pid_t>>();
                self.bookmarks.clear_marks();
                Interaction::Narrow(pids)
            }
            Action::ChangeScope => Interaction::Wide,
            Action::FilterNone | Action::FilterUser | Action::FilterActive => {
                Interaction::Filter(self.filter)
            }
            Action::SwitchToProcess => match self.bookmarks.selected() {
                Some(selected) => Interaction::SelectPid(selected.pid),
                None => Interaction::None,
            },
            _ => Interaction::None,
        })
    }

    /// Make the row of headers.
    ///
    /// The first column is the name. The second is the PID. The rest are the metrics.
    fn make_header_row<'p>(
        hoffset: usize,
        cws: &mut [MaxLength],
        metric_headers: Vec<Text<'p>>,
    ) -> Vec<Cell<'p>> {
        let column_count = cws.len();
        let mut row = Vec::with_capacity(column_count);
        row.push(Cell::from(""));

        const PID_TITLE: &str = "PID";
        row.push(Cell::from(
            Text::from(PID_TITLE).alignment(Alignment::Center),
        ));
        let mut col_index = 1;
        cws[col_index].check(PID_TITLE);
        col_index += 1;

        const STATE_TITLE: &str = "S";
        row.push(Cell::from(Text::from(STATE_TITLE)));
        cws[col_index].check(STATE_TITLE);
        col_index += 1;

        metric_headers
            .iter()
            .skip(hoffset)
            .enumerate()
            .for_each(|(index, text)| {
                text.iter().for_each(|line| {
                    let line_len = line.iter().map(|span| span.content.len()).sum();
                    cws[col_index + index].set_min(line_len)
                });
                row.push(Cell::from(text.clone().alignment(Alignment::Center)));
            });
        row
    }

    /// Make a row of metrics.
    ///
    /// The first column is the name. The second is the PID. The rest are the metrics.
    fn make_metrics_row<'p>(
        name_status: PidStatus,
        hoffset: usize,
        indent: usize,
        cws: &mut [MaxLength],
        ps: &'p ProcessSamples,
        styles: &Styles,
    ) -> Vec<Cell<'p>> {
        let column_count = cws.len();
        let mut row = Vec::with_capacity(column_count);
        let name_style = styles.name_style(name_status);
        let name = {
            let name = ps.name();
            format!("{:>width$}", name, width = indent + name.len())
        };
        let mut col_index = 0;
        cws[col_index].check(&name);
        col_index += 1;
        row.push(Cell::from(name).style(name_style));
        let pid = format!("{}", ps.pid());
        cws[col_index].check(&pid);
        col_index += 1;
        row.push(rcell!(pid));
        col_index += 1;
        row.push(rcell!(ps.state().to_string()));
        ps.samples()
            .flat_map(|sample| izip!(sample.strings(), sample.trends()))
            .skip(hoffset)
            .for_each(|(value, trend)| {
                cws[col_index].check(value);
                col_index += 1;
                row.push(Cell::from(
                    Text::from(value.as_str())
                        .style(styles.trend_style(trend))
                        .alignment(Alignment::Right),
                ));
            });
        row
    }

    /// Make a row of limits.
    ///
    /// The first column is the name. The second is the PID. The rest are the metrics.
    fn make_limits_row<'p>(
        hoffset: usize,
        cws: &mut [MaxLength],
        ps: &'p ProcessSamples,
        display_limits: LimitKind,
        limit_slots: &[bool],
    ) -> Vec<Cell<'p>> {
        let column_count = cws.len();
        let mut row = Vec::with_capacity(column_count);
        let limits_title = match display_limits {
            LimitKind::None => "no limit",
            LimitKind::Soft => "soft limits",
            LimitKind::Hard => "hard limits",
        };
        cws[0].check(limits_title);
        row.push(rcell!(limits_title));
        row.push(Cell::new(""));
        let mut col_index = 0;
        const NOT_APPLICABLE: &str = "n/a";
        ps.samples().for_each(|sample| {
            let max_index = col_index + sample.string_count();
            if col_index >= hoffset {
                while col_index < max_index {
                    if limit_slots[hoffset + col_index] {
                        let text = sample.limit(display_limits.clone()).unwrap_or("--");
                        cws[col_index].check(text);
                        row.push(rcell!(text));
                    } else {
                        row.push(rcell!(NOT_APPLICABLE));
                    }
                    col_index += 1;
                }
            }
        });
        row
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
        let line_count = collector.line_count();
        let ncols = self.metric_headers.len() + 3; // process name, PID, state, metric1, ...
        let nrows = line_count + 2; // metric title, metric subtitle, process1, ...
        let top = self.top(line_count);
        let voffset = self.bookmarks.execute(
            &mut self.occurrences,
            collector.lines().skip(1),
            top,
            self.body_height,
        );
        self.table_offset.set_vertical(voffset);
        let (hoffset, voffset) = self
            .table_offset
            .set_bounds(ncols - 3, line_count.saturating_sub(self.body_height));
        let nvisible_rows = nrows - voffset;
        let nvisible_cols = ncols - hoffset;

        let mut cws = Vec::with_capacity(nvisible_cols); // column widths
        cws.resize(nvisible_cols, MaxLength::default());

        let metric_headers = self.metric_headers.clone();
        let headers = TerminalDevice::make_header_row(hoffset, &mut cws, metric_headers);

        let mut pids = PidStack::default();
        collector
            .lines()
            .take(voffset)
            .for_each(|sample| pids.push(sample));

        let with_limits = matches!(self.display_limits, LimitKind::Soft | LimitKind::Hard);
        let display_limits = self.display_limits.clone();
        let rows = {
            let mut rows: Vec<Vec<Cell>> = Vec::with_capacity(nvisible_rows);
            collector.lines().skip(voffset).for_each(|samples| {
                pids.push(samples);
                let pid = samples.pid();
                let pid_status = self.pid_status(pid);

                let row = TerminalDevice::make_metrics_row(
                    pid_status,
                    hoffset,
                    pids.len().saturating_sub(1), // indent
                    &mut cws,
                    samples,
                    &self.styles,
                );
                rows.push(row);
                if matches!(pid_status, PidStatus::Selected) && with_limits {
                    let row = TerminalDevice::make_limits_row(
                        hoffset,
                        &mut cws,
                        samples,
                        display_limits.clone(),
                        &self.limit_slots,
                    );
                    rows.push(row);
                }
            });
            rows
        };

        let col_widths = cws.iter().map(MaxLength::len).collect::<Vec<u16>>();
        self.draw_tree(headers, rows, &col_widths)?;
        Ok(())
    }

    fn render_help(&mut self) -> anyhow::Result<()> {
        self.pane_kind = PaneKind::Help;
        let offset = self.pane_offset;
        let mut main = MarkdownWidget::new("OPRS", HELP, offset);
        let menu = OneLineWidget::with_menu(self.menu.iter(), self.keymap);

        self.terminal.draw(|frame| {
            let mut rects = SingleScrollablePane::new(frame.area(), 2)
                .with(&menu)
                .build();

            if let Some(Some(main_rect)) = rects.first_mut() {
                let (inner_height, offset) = main.prepare(main_rect);
                self.pane_offset = offset;
                self.vertical_scroll = inner_height.div_ceil(2) as usize;
            }
            let mut r = OptionalRenderer::new(frame, &mut rects);
            r.render_widget(main);
            r.render_widget(menu);
        })?;
        Ok(())
    }

    fn format_option<D: fmt::Display>(option: Option<D>) -> String {
        match option {
            Some(value) => value.to_string(),
            None => "<unknown>".to_string(),
        }
    }

    fn render_details(&mut self, details: &ProcessDetails) -> anyhow::Result<()> {
        self.pane_kind = PaneKind::Process;
        let pinfo = details.process();
        let cmdline = pinfo.cmdline();
        let metrics = details.metrics();

        let cmdline_widget =
            OneLineWidget::new(Text::from(cmdline), Style::default(), Some("Command"));
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
        let menu = OneLineWidget::with_menu(self.menu.iter(), self.keymap);

        self.terminal.draw(|frame| {
            let mut rects = GridPane::new(frame.area())
                .with_row(&[&cmdline_widget])
                .with_row(&[&proc_widget, &file_widget])
                .with_row(&[&cpu_widget, &mem_widget])
                .with_line(&menu)
                .build();
            let mut r = OptionalRenderer::new(frame, &mut rects);
            r.render_widget(cmdline_widget);
            r.render_widget(proc_widget);
            r.render_widget(file_widget);
            r.render_widget(cpu_widget);
            r.render_widget(mem_widget);
            r.render_widget(Clear);
            r.render_widget(menu);
        })?;
        self.vertical_scroll = 1; // scrolling by block not by line.
        Ok(())
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
            self.headers_height = std::cmp::max(self.headers_height, header.len());
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
            self.metric_headers.push(Text::from(
                header
                    .iter()
                    .map(|s| Line::from(s.to_string()))
                    .collect::<Vec<Line>>(),
            ));
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
    fn render(&mut self, pane: PaneData, _redraw: bool) -> anyhow::Result<()> {
        match pane {
            PaneData::Main(collector) => {
                let is_incremental_search = self.bookmarks.is_incremental_search();
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
            PaneData::Process(details) => {
                self.set_keymap(KeyMap::Details);
                self.render_details(details)
            }
            PaneData::Help => {
                self.set_keymap(KeyMap::Help);
                self.render_help()
            }
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
