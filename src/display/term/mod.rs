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
    layout::{Alignment, Constraint, Direction, Layout, Position, Rect, Size},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
    Frame, Terminal,
};
use std::{cmp::Ordering, fmt, io, time::Duration};
use termion::{
    raw::{IntoRawMode, RawTerminal},
    screen::{AlternateScreen, IntoAlternateScreen},
};

use crate::{
    clock::Timer,
    console::{is_tty, BuiltinTheme, EventChannel},
    process::{
        format::human_duration, Aggregation, Collector, FormattedMetric, LimitKind, ProcessDetails,
        ProcessIdentity, ProcessSamples,
    },
};
use num_traits::Zero;

use super::{DisplayDevice, Pane, PaneKind, PauseStatus, SliceIter};

mod input;

#[macro_use]
mod types;

use input::{menu, Action, BookmarkAction, Bookmarks, KeyMap, MenuEntry, PidStatus, SearchEdit};
use types::{Area, UnboundedArea};

/// Right aligned cell.
macro_rules! rcell {
    ($s:expr) => {
        Cell::from(Text::from($s).alignment(Alignment::Right))
    };
}

const HELP: &str = r#"Command help
============

Movements
---------

- Up and down: move the selection up and down.
- Page up and down: scroll by pages.
- Control-Home: go to first line.
- Control-End: go to last line.
- f: Go back to the selected line.
- Left and Right: move the columns left or right.
- Home: go to first column.
- End: go to last column.

Searching
---------

Start an incremental search with '/'. Hit enter to validate the search string.

Move to the next match with 'n' and the previous match with 'N'.

Miscellaneous
-------------

The soft or hard limits are displayed by hitting 'l' but only for the selected process.

By default, only userland processes are displayed. Use 'F' to change.
"#;

/// User action that has an impact on the application.
#[derive(Clone, Copy, Debug)]
pub enum Interaction {
    None,
    Filter(usize),
    SwitchToHelp,
    SwitchBack,
    SelectPid(pid_t),
    Quit,
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
            None => stack.clear(),
        }
    }
}

/// A list of filters with a current value.
pub struct FilterLoop {
    /// Filter names
    names: Vec<&'static str>,
    /// Current filter
    current: usize,
}

impl FilterLoop {
    pub fn new(names: &[&'static str], current: usize) -> Self {
        Self {
            names: names.to_vec(),
            current,
        }
    }

    fn current_name(&self) -> &'static str {
        self.names[self.current]
    }

    fn advance(&mut self) {
        self.current += 1;
        if self.current >= self.names.len() {
            self.current = 0;
        }
    }
}

/// Navigation arrows dependending on table overflows
///
/// # Arguments
///
/// * `shifted` - Area boolean saying if the first line or first column are hidden.
/// * `overflow` - Area boolean saying if the end is visible.
fn navigation_arrows(shifted: Area<bool>, overflows: Area<bool>) -> Text<'static> {
    let up_arrow = if shifted.vertical { " " } else { "⬆" };
    let down_arrow = if overflows.vertical { "⬇" } else { " " };
    let left_arrow = if shifted.horizontal { " " } else { "⬅" };
    let right_arrow = if overflows.horizontal { "➡" } else { " " };
    Text::from(vec![
        Line::from(up_arrow),
        Line::from(format!("{left_arrow} {down_arrow} {right_arrow}")),
    ])
    .alignment(Alignment::Center)
}

/// Apply style to rows
///
/// The table is truncated to keep only `ncols` column.
fn style_rows<'a>(
    rows: &mut Vec<Vec<Cell<'a>>>,
    ncols: usize,
    even_row_style: Style,
    odd_row_style: Style,
) -> Vec<Row<'a>> {
    rows.drain(..)
        .enumerate()
        .map(|(i, mut r)| {
            let style = if i % 2 != 0 {
                even_row_style
            } else {
                odd_row_style
            };
            if r.len() < ncols {
                panic!("rows must have {} columns instead of {}", ncols, r.len());
            }
            Row::new(r.drain(0..ncols)).style(style)
        })
        .collect::<Vec<Row>>()
}

/// Calculate widths constraints to avoid an overflow
///
/// # Arguments
///
/// * `inner_width` - The usable width to display the table.
fn width_constraints(
    inner_width: u16,
    column_widths: &[u16],
    column_spacing: u16,
) -> (u16, Vec<Constraint>, bool) {
    let mut total_width = 0;
    let mut constraints = Vec::with_capacity(column_widths.len());
    let mut current_column_spacing = 0;
    let mut truncated = false;
    while constraints.len() < column_widths.len() {
        let index = constraints.len();
        let col_width = column_widths[index];
        let new_total_width = total_width + current_column_spacing + col_width;
        if new_total_width <= inner_width {
            constraints.push(Constraint::Length(col_width));
        } else {
            let remaining = inner_width - total_width;
            if remaining > column_spacing {
                // Partial last column
                constraints.push(Constraint::Length(remaining - column_spacing));
                total_width = inner_width;
                truncated = true;
            }
            break;
        }
        total_width = new_total_width;
        current_column_spacing = column_spacing;
    }
    let hoverflow = constraints.len() < column_widths.len() || truncated;
    (total_width, constraints, hoverflow)
}

fn menu_line(entries: &[MenuEntry], keymap: KeyMap) -> Text<'static> {
    let mut spans = Vec::new();
    let mut sep = "";
    entries
        .iter()
        .filter(|e| e.keymap.intersects(keymap))
        .for_each(|entry| {
            spans.push(Span::raw(sep));
            spans.push(Span::styled(
                entry.key().to_string(),
                Style::default().add_modifier(Modifier::REVERSED),
            ));
            spans.push(Span::raw(format!(" {}", entry.label())));
            sep = "  ";
        });
    Text::from(Line::from(spans))
}

/// Compute the maximum length of strings
#[derive(Clone, Copy, Debug, Default)]
struct MaxLength(u16);

impl MaxLength {
    fn with_lines<'a, I>(items: I) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        let mut ml = MaxLength(0);
        for item in items.into_iter() {
            ml.check(item);
        }
        ml
    }

    /// The length:
    fn len(&self) -> u16 {
        let Self(length) = self;
        *length
    }

    /// Count the maximun length of a string
    fn check(&mut self, s: &str) {
        self.set_min(s.len());
    }

    /// Ensure a minimum length
    fn set_min(&mut self, l: usize) {
        let l = l as u16;
        if l > self.0 {
            self.0 = l
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
    /// Filters
    filters: FilterLoop,
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
    /// Menu
    menu: Vec<MenuEntry>,
    /// Pane kind.
    pane_kind: PaneKind,
    /// Help height
    help_height: usize,
}

impl<'t> TerminalDevice<'t> {
    pub fn new(
        every: Duration,
        theme: Option<BuiltinTheme>,
        filters: FilterLoop,
    ) -> anyhow::Result<Self> {
        let screen = io::stdout().into_raw_mode()?.into_alternate_screen()?;
        let backend = TermionBackend::new(Box::new(screen));
        let terminal = Terminal::new(backend)?;

        Ok(TerminalDevice {
            every,
            filters,
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
            menu: menu(),
            pane_kind: PaneKind::Main,
            help_height: HELP.lines().count(),
        })
    }

    pub fn is_available() -> bool {
        is_tty(&io::stdin())
    }

    fn keymap(&self) -> KeyMap {
        match self.pane_kind {
            PaneKind::Help => KeyMap::Help,
            PaneKind::Process => KeyMap::Details,
            PaneKind::Main if self.bookmarks.is_incremental_search() => KeyMap::IncrementalSearch,
            PaneKind::Main if self.bookmarks.is_search() => KeyMap::FixedSearch,
            PaneKind::Main => KeyMap::Main,
        }
    }

    /// Content of the status bar
    fn status_bar(&self) -> String {
        let time_string = format!("{}", Local::now().format("%X"));
        let delay = human_duration(self.every);
        format!(
            "{time_string} -- interval:{delay} -- limit:{} -- filter:{}",
            self.display_limits.as_ref(),
            self.filters.current_name()
        )
    }

    /// Draw the table of metrics and the menu.
    ///
    /// Return the table visible height.
    fn draw_tree(
        &mut self,
        mut headers: Vec<Cell>,
        mut rows: Vec<Vec<Cell>>,
        nrows: usize,
        col_widths: &[u16],
    ) -> anyhow::Result<()> {
        let offset = self.table_offset;
        let mut new_overflow = Area::default();
        let even_row_style = self.styles.even_row;
        let odd_row_style = self.styles.odd_row;
        let mut body_height = 0;
        let headers_height = self.headers_height as u16;
        let status_bar = Paragraph::new(Text::from(self.status_bar())).style(self.styles.status);
        let is_search = self.bookmarks.is_incremental_search();
        let show_cursor = is_search;
        let menu = if is_search {
            Text::from(format!(
                "Search: {}",
                self.bookmarks.search_pattern().unwrap()
            ))
        } else {
            menu_line(&self.menu, self.keymap())
        };

        self.terminal.draw(|frame| {
            const BORDERS_SIZE: u16 = 2;
            const MENU_HEIGHT: u16 = 1;
            const STATUS_HEIGHT: u16 = 1;
            const FOOTER_HEIGHT: u16 = MENU_HEIGHT + STATUS_HEIGHT;
            let screen = frame.area();
            let outter_area = Size::new(screen.width, screen.height - FOOTER_HEIGHT);
            let inner_area = Size::new(
                outter_area.width - BORDERS_SIZE,
                outter_area.height - BORDERS_SIZE,
            );
            let rects = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    [
                        Constraint::Length(outter_area.height),
                        Constraint::Min(0),
                        Constraint::Min(0),
                    ]
                    .as_ref(),
                )
                .split(screen);
            let column_spacing = self.styles.column_spacing;
            let (_table_width, widths, hoverflow) =
                width_constraints(inner_area.width, col_widths, self.styles.column_spacing);
            let table_height = headers_height + nrows as u16;
            new_overflow = Area::new(hoverflow, table_height > inner_area.height);
            let shifted = Area::new(offset.horizontal.is_zero(), offset.vertical.is_zero());
            let nav = navigation_arrows(shifted, new_overflow);
            headers[0] = Cell::from(nav);

            let rows = style_rows(&mut rows, widths.len(), even_row_style, odd_row_style);
            let table = Table::new(rows, widths)
                .block(Block::default().borders(Borders::ALL))
                .header(Row::new(headers).height(headers_height))
                .column_spacing(column_spacing);

            let cursor_pos = Position::new(menu.width() as u16, screen.height - 1);
            frame.render_widget(table, rects[0]);
            frame.render_widget(status_bar, rects[1]);
            frame.render_widget(Paragraph::new(menu), rects[2]);

            if show_cursor {
                frame.set_cursor_position(cursor_pos);
            }
            body_height = inner_area.height - headers_height;
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
            Action::None | Action::Quit | Action::SwitchToHelp | Action::SwitchToProcess => {}
            Action::SwitchBack => self.pane_offset = 0,
            Action::FilterNext => self.filters.advance(),
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
            Action::ScrollRight => {
                if self.overflow.horizontal {
                    self.table_offset.scroll_right(1);
                }
            }
            Action::ScrollUp => match self.pane_kind {
                PaneKind::Main => {
                    self.bookmarks.clear_search();
                    self.table_offset.scroll_up(self.vertical_scroll);
                }
                _ => {
                    self.pane_offset = self.pane_offset.saturating_sub(self.vertical_scroll as u16);
                }
            },
            Action::ScrollDown => match self.pane_kind {
                PaneKind::Main => {
                    self.bookmarks.clear_search();
                    if self.overflow.vertical {
                        self.table_offset.scroll_down(self.vertical_scroll);
                    }
                }
                _ => {
                    self.pane_offset += self.vertical_scroll as u16;
                }
            },
            Action::ScrollLeft => self.table_offset.scroll_left(1),
            Action::SelectUp => {
                self.bookmarks.clear_search();
                void!(self.bookmarks.set_action(BookmarkAction::PreviousLine));
            }
            Action::SelectDown => {
                self.bookmarks.clear_search();
                void!(self.bookmarks.set_action(BookmarkAction::NextLine))
            }
            Action::Focus => void!(self.bookmarks.set_action(BookmarkAction::Focus)),
            Action::GotoTableTop => self.table_offset.home(),
            Action::GotoTableBottom => self.table_offset.vertical_end(),
            Action::GotoTableLeft => self.table_offset.horizontal_home(),
            Action::GotoTableRight => self.table_offset.horizontal_end(),
            Action::SearchEnter => self.bookmarks.incremental_search(),
            Action::SearchExit => {
                self.terminal.hide_cursor()?;
                self.bookmarks.fixed_search()
            }
            Action::SearchPush(c) => self.bookmarks.edit_search(SearchEdit::Push(c)),
            Action::SearchPop => self.bookmarks.edit_search(SearchEdit::Pop),
            Action::SearchPrevious => {
                void!(self.bookmarks.set_action(BookmarkAction::PreviousMatch))
            }
            Action::SearchNext => void!(self.bookmarks.set_action(BookmarkAction::NextMatch)),
        }
        Ok(action)
    }

    /// Convert the action to a possible interaction.
    fn interaction(&self, action: Action) -> Interaction {
        match action {
            Action::FilterNext => Interaction::Filter(self.filters.current),
            Action::SwitchToHelp => Interaction::SwitchToHelp,
            Action::SwitchToProcess => match self.bookmarks.selected() {
                Some(pid) => Interaction::SelectPid(*pid),
                None => Interaction::None,
            },
            Action::SwitchBack => Interaction::SwitchBack,
            Action::Quit => Interaction::Quit,
            _ => Interaction::None,
        }
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

    fn render_tree(&mut self, collector: &Collector) -> anyhow::Result<()> {
        self.pane_kind = PaneKind::Main;
        let line_count = collector.line_count();
        let ncols = self.metric_headers.len() + 2; // process name, PID, metric1, ...
        let nrows = line_count + 2; // metric title, metric subtitle, process1, ...
        let selected_lineno = match self.bookmarks.execute(
            collector.lines(),
            self.table_offset.vertical,
            self.body_height,
        ) {
            (Some(lineno), Some(voffset)) => {
                self.table_offset.set_vertical(voffset);
                Some(lineno)
            }
            (Some(lineno), None) => Some(lineno),
            (None, Some(voffset)) => {
                self.table_offset.set_vertical(voffset);
                None
            }
            (None, None) => None,
        };
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
            collector
                .lines()
                .enumerate()
                .skip(voffset)
                .for_each(|(lineno, samples)| {
                    pids.push(samples);
                    let is_selected = match selected_lineno {
                        Some(selected_lineno) => selected_lineno == lineno,
                        None => false,
                    };
                    let row = TerminalDevice::make_metrics_row(
                        self.bookmarks.status(samples.pid()),
                        hoffset,
                        pids.len().saturating_sub(1),
                        &mut cws,
                        samples,
                        &self.styles,
                    );
                    rows.push(row);
                    if is_selected && with_limits {
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
        self.draw_tree(headers, rows, nvisible_rows, &col_widths)?;
        Ok(())
    }

    fn render_help(&mut self) -> anyhow::Result<()> {
        self.pane_kind = PaneKind::Help;
        let menu = menu_line(&self.menu, self.keymap());
        let help_height = self.help_height as u16;
        let mut pane_offset = self.pane_offset;

        self.terminal.draw(|frame| {
            const BORDERS_SIZE: u16 = 2;
            const MENU_HEIGHT: u16 = 1;
            let screen = frame.area();
            let body_height = screen.height - BORDERS_SIZE - MENU_HEIGHT;
            let rects = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(body_height), Constraint::Min(0)].as_ref())
                .split(screen);
            let inner_height = body_height - BORDERS_SIZE;
            let max_pane_offset = help_height.saturating_sub(inner_height / 2);
            if pane_offset > max_pane_offset {
                pane_offset = max_pane_offset;
            }
            let help = Paragraph::new(HELP)
                .block(
                    Block::new()
                        .title(" Oprs ")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: false })
                .scroll((pane_offset, 0));
            frame.render_widget(help, rects[0]);
            frame.render_widget(Paragraph::new(menu), rects[1]);
            self.vertical_scroll = body_height.div_ceil(2) as usize;
        })?;
        self.pane_offset = pane_offset;
        Ok(())
    }

    fn render_fields(
        frame: &mut Frame<'_>,
        area: Rect,
        title: &str,
        lines: &[(&'static str, String)],
    ) {
        let rows = lines.iter().map(|(name, value)| {
            Row::new(vec![
                Text::from(name.to_string()),
                Text::from(value.to_string()).alignment(Alignment::Right),
            ])
        });
        let cw1 = MaxLength::with_lines(lines.iter().map(|(name, _)| *name));
        let constraints = [Constraint::Length(cw1.len()), Constraint::Min(0)];
        let table = Table::new(rows, constraints).block(
            Block::new()
                .title(title)
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL),
        );
        frame.render_widget(table, area);
    }

    fn format_option<D: fmt::Display>(option: Option<D>) -> String {
        match option {
            Some(value) => value.to_string(),
            None => "<unknown>".to_string(),
        }
    }

    fn format_result<D: fmt::Display, E>(result: Result<D, E>) -> String {
        TerminalDevice::format_option(result.ok())
    }

    fn render_details(&mut self, details: &ProcessDetails) -> anyhow::Result<()> {
        self.pane_kind = PaneKind::Process;
        let menu = menu_line(&self.menu, KeyMap::Details);
        let pane_offset = self.pane_offset;

        let process = details.process();
        let cmdline = process
            .cmdline()
            .map(|v| v.join(" "))
            .unwrap_or_else(|_| String::from("<zombie>"));
        let metrics = details.metrics();
        let proc_info = &[
            ("Name", format!(" {} ", details.process_name())),
            ("PID", format!("{}", process.pid())),
            ("Owner", TerminalDevice::format_result(process.uid())),
            ("Threads", format_metric!(metrics, thread_count)),
        ];
        let file_info = &[
            ("Descriptors", format_metric!(metrics, fd_all)),
            ("Files", format_metric!(metrics, fd_file)),
            ("I/O Read", format_metric!(metrics, io_read_total)),
            ("I/O Write", format_metric!(metrics, io_write_total)),
        ];
        let cpu_info = &[
            ("CPU", format_metric!(metrics, time_cpu)),
            ("Elapsed", format_metric!(metrics, time_elapsed)),
        ];
        let mem_info = &[
            ("VM", format_metric!(metrics, mem_vm)),
            ("RSS", format_metric!(metrics, mem_rss)),
            ("Data", format_metric!(metrics, mem_data)),
        ];

        self.terminal.draw(|frame| {
            const BORDERS_SIZE: u16 = 2;
            //const MENU_HEIGHT: u16 = 1;
            let screen = frame.area();
            let inner_width = screen.width - BORDERS_SIZE;
            let block1_height = (cmdline.len() as u16).div_ceil(inner_width) + BORDERS_SIZE;
            let block2_height =
                std::cmp::max(proc_info.len(), file_info.len()) as u16 + BORDERS_SIZE;
            let block3_height = std::cmp::max(cpu_info.len(), mem_info.len()) as u16 + BORDERS_SIZE;

            let rects = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    [
                        Constraint::Length(block1_height),
                        Constraint::Length(block2_height),
                        Constraint::Length(block3_height),
                        Constraint::Min(0),
                    ]
                    .as_ref(),
                )
                .split(screen);
            let cmdline = Paragraph::new(cmdline)
                .block(
                    Block::new()
                        .title(" Command Line ")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: false });
            frame.render_widget(cmdline, rects[0]);

            let two_cols_constraint = &[Constraint::Percentage(50), Constraint::Percentage(50)];
            let block2_rects = Layout::horizontal(two_cols_constraint).split(rects[1]);
            TerminalDevice::render_fields(frame, block2_rects[0], "Process", proc_info);
            TerminalDevice::render_fields(frame, block2_rects[1], "Files", file_info);
            let block3_rects = Layout::horizontal(two_cols_constraint).split(rects[2]);
            TerminalDevice::render_fields(frame, block3_rects[0], "Time", cpu_info);
            TerminalDevice::render_fields(frame, block3_rects[1], "Memory", mem_info);
            frame.render_widget(Paragraph::new(menu), rects[3]);
        })?;
        self.pane_offset = pane_offset;
        self.vertical_scroll = 1; // scrolling by block not by line.
        Ok(())
    }
}

impl<'t> DisplayDevice for TerminalDevice<'t> {
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
    fn render(&mut self, pane: Pane, _redraw: bool) -> anyhow::Result<()> {
        match pane {
            Pane::Main(collector) => self.render_tree(collector),
            Pane::Process(details) => self.render_details(details),
            Pane::Help => self.render_help(),
        }
    }

    /// Wait for a user input or a timeout.
    fn pause(&mut self, timer: &mut Timer) -> anyhow::Result<PauseStatus> {
        if let Some(timeout) = timer.remaining() {
            if let Some(evt) = self.events.receive_timeout(timeout)? {
                let keymap = self.keymap();
                let action = self.react(keymap.action_from_event(evt), timer)?;
                Ok(PauseStatus::Action(self.interaction(action)))
            } else {
                Ok(PauseStatus::TimeOut)
            }
        } else {
            Ok(PauseStatus::TimeOut)
        }
    }
}

#[cfg(test)]
mod test {

    use ratatui::layout::Constraint;

    use super::width_constraints;

    #[test]
    fn test_width_constraints_underflow() {
        // 0         1
        // 01234567890123456789
        // aaaaa bbbb bbbb
        const SCREEN_WIDTH: u16 = 20;
        const FIRST_COLUMN_WIDTH: u16 = 5;
        const COLUMN_WIDTH: u16 = 4;
        let column_widths = vec![FIRST_COLUMN_WIDTH, COLUMN_WIDTH, COLUMN_WIDTH];
        const COLUMN_SPACING: u16 = 1;
        const NCOLS: usize = 3;
        let (table_width, widths, hoverflow) =
            width_constraints(SCREEN_WIDTH, &column_widths, COLUMN_SPACING);
        const SPACED_COLUMN_WIDTH: u16 = COLUMN_WIDTH + COLUMN_SPACING;
        const EXPECTED_WIDTH: u16 = FIRST_COLUMN_WIDTH + (NCOLS as u16 - 1) * SPACED_COLUMN_WIDTH;
        assert_eq!(EXPECTED_WIDTH, table_width);
        assert_eq!(NCOLS, widths.len());
        assert_eq!(Constraint::Length(FIRST_COLUMN_WIDTH), widths[0]);
        assert_eq!(Constraint::Length(COLUMN_WIDTH), widths[1]);
        assert!(!hoverflow);
    }

    #[test]
    fn test_width_constraints_exact() {
        // 0         1
        // 01234567890123456789
        // aaaaa bbbb bbbb bbbb
        const SCREEN_WIDTH: u16 = 20;
        const FIRST_COLUMN_WIDTH: u16 = 5;
        const COLUMN_WIDTH: u16 = 4;
        let column_widths = vec![FIRST_COLUMN_WIDTH, COLUMN_WIDTH, COLUMN_WIDTH, COLUMN_WIDTH];
        const COLUMN_SPACING: u16 = 1;
        let (table_width, widths, hoverflow) =
            width_constraints(SCREEN_WIDTH, &column_widths, COLUMN_SPACING);
        const SPACED_COLUMN_WIDTH: u16 = COLUMN_WIDTH + COLUMN_SPACING;
        let expected_width: u16 =
            FIRST_COLUMN_WIDTH + (column_widths.len() as u16 - 1) * SPACED_COLUMN_WIDTH;
        const EXPECTED_NCOLS: usize = 4;
        assert_eq!(expected_width, table_width);
        assert_eq!(EXPECTED_NCOLS, widths.len());
        assert_eq!(Constraint::Length(FIRST_COLUMN_WIDTH), widths[0]);
        assert_eq!(Constraint::Length(COLUMN_WIDTH), widths[1]);
        assert!(!hoverflow);
    }

    #[test]
    fn test_width_constraints_overflow() {
        // 0         1
        // 01234567890123456789
        // aaaaa bbb bbb bbb bbb bbb
        const SCREEN_WIDTH: u16 = 20;
        const FIRST_COLUMN_WIDTH: u16 = 5;
        const COLUMN_WIDTH: u16 = 3;
        let column_widths = vec![
            FIRST_COLUMN_WIDTH,
            COLUMN_WIDTH,
            COLUMN_WIDTH,
            COLUMN_WIDTH,
            COLUMN_WIDTH,
            COLUMN_WIDTH,
        ];
        const COLUMN_SPACING: u16 = 1;
        let (table_width, widths, hoverflow) =
            width_constraints(SCREEN_WIDTH, &column_widths, COLUMN_SPACING);
        const EXPECTED_NCOLS: usize = 5;
        assert_eq!(SCREEN_WIDTH, table_width);
        assert_eq!(EXPECTED_NCOLS, widths.len());
        assert_eq!(Constraint::Length(FIRST_COLUMN_WIDTH), widths[0]);
        assert_eq!(Constraint::Length(COLUMN_WIDTH), widths[1]);
        assert_eq!(Constraint::Length(2), widths[4]);
        assert!(hoverflow);
    }
}
