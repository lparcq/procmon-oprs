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
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Terminal,
};
use std::cmp::Ordering;
use std::io;
use std::time::Duration;
use termion::{
    raw::{IntoRawMode, RawTerminal},
    screen::{AlternateScreen, IntoAlternateScreen},
};

use crate::{
    agg::Aggregation,
    clock::Timer,
    collector::{Collector, LimitKind, ProcessSamples},
    console::{is_tty, BuiltinTheme, Event, EventChannel, Key},
    format::human_duration,
    metrics::FormattedMetric,
};

use super::{DisplayDevice, PauseStatus, SliceIter};

/// Right aligned cell.
macro_rules! rcell {
    ($s:expr) => {
        Cell::from(Text::from($s).alignment(Alignment::Right))
    };
}

/// Area property with an horizontal and vertical value.
#[derive(Clone, Copy, Debug, Default)]
struct AreaProperty<T: Default> {
    horizontal: T,
    vertical: T,
}

impl<T: Default> AreaProperty<T> {
    fn new(horizontal: T, vertical: T) -> Self {
        Self {
            horizontal,
            vertical,
        }
    }
}

impl AreaProperty<usize> {
    fn scroll_left(&mut self, delta: usize) {
        self.horizontal = self.horizontal.saturating_sub(delta);
    }

    fn scroll_right(&mut self, delta: usize) {
        self.horizontal += delta;
    }

    fn scroll_up(&mut self, delta: usize) {
        self.vertical = self.vertical.saturating_sub(delta);
    }

    fn scroll_down(&mut self, delta: usize) {
        self.vertical += delta;
    }
}

/// Theme styles
struct Styles {
    even_row: Style,
    odd_row: Style,
    increase: Style,
    decrease: Style,
    unselected: Style,
    selected: Style,
    column_spacing: u16,
}

impl Styles {
    fn new(theme: Option<BuiltinTheme>) -> Self {
        let default_style = Style::default();
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let bold_reversed = bold.add_modifier(Modifier::REVERSED);
        match theme {
            Some(BuiltinTheme::Dark) => Styles {
                even_row: default_style,
                odd_row: Style::default().bg(Color::Rgb(40, 40, 40)),
                increase: Style::default().fg(Color::Rgb(235, 45, 83)),
                decrease: Style::default().fg(Color::Rgb(166, 255, 77)),
                unselected: bold,
                selected: bold_reversed,
                column_spacing: 2,
            },
            Some(BuiltinTheme::Light) => Styles {
                even_row: default_style,
                odd_row: Style::default().bg(Color::Rgb(215, 215, 215)),
                increase: Style::default().fg(Color::Rgb(220, 20, 60)),
                decrease: Style::default().fg(Color::Rgb(102, 204, 00)),
                unselected: bold,
                selected: bold_reversed,
                column_spacing: 2,
            },
            Some(BuiltinTheme::Dark16) => Styles {
                even_row: default_style,
                odd_row: Style::default().fg(Color::LightBlue),
                increase: Style::default().fg(Color::LightMagenta),
                decrease: Style::default().fg(Color::LightGreen),
                unselected: bold,
                selected: bold_reversed,
                column_spacing: 2,
            },
            Some(BuiltinTheme::Light16) => Styles {
                even_row: default_style,
                odd_row: Style::default().bg(Color::Blue),
                increase: Style::default().fg(Color::Red),
                decrease: Style::default().fg(Color::Green),
                unselected: bold,
                selected: bold_reversed,
                column_spacing: 2,
            },
            None => Styles {
                even_row: default_style,
                odd_row: default_style,
                increase: bold,
                decrease: bold,
                unselected: bold,
                selected: bold_reversed,
                column_spacing: 2,
            },
        }
    }

    fn name_style(&self, is_selected: bool) -> Style {
        if is_selected {
            self.selected
        } else {
            self.unselected
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
                loop {
                    if let Some(top_pid) = stack.last() {
                        if *top_pid == parent_pid {
                            break;
                        }
                        let _ = stack.pop();
                    } else {
                        break;
                    }
                }
                stack.push(samples.pid());
            }
            None => stack.clear(),
        }
    }
}

impl Default for PidStack {
    fn default() -> Self {
        PidStack(Vec::new())
    }
}

/// Standard keys
const KEY_QUIT: Key = Key::Esc;
const KEY_FASTER_CHAR: char = '+';
const KEY_FASTER: Key = Key::Char(KEY_FASTER_CHAR);
const KEY_SLOWER_CHAR: char = '-';
const KEY_SLOWER: Key = Key::Char(KEY_SLOWER_CHAR);
const KEY_LIMITS_UPPER: Key = Key::Char('L');
const KEY_LIMITS_LOWER: Key = Key::Char('l');

/// Action
pub enum Action {
    None,
    DivideTimeout(u16),
    MultiplyTimeout(u16),
    ToggleLimits,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
    ScrollUp,
    SelectUp,
    SelectDown,
    Quit,
}

impl From<Event> for Action {
    fn from(evt: Event) -> Self {
        match evt {
            Event::Key(KEY_QUIT) => Action::Quit,
            Event::Key(Key::Ctrl('c')) => Action::Quit,
            Event::Key(KEY_FASTER) => Action::DivideTimeout(2),
            Event::Key(KEY_SLOWER) => Action::MultiplyTimeout(2),
            Event::Key(KEY_LIMITS_UPPER) | Event::Key(KEY_LIMITS_LOWER) => Action::ToggleLimits,
            Event::Key(Key::Right) => Action::ScrollRight,
            Event::Key(Key::PageUp) => Action::ScrollUp,
            Event::Key(Key::PageDown) => Action::ScrollDown,
            Event::Key(Key::Left) => Action::ScrollLeft,
            Event::Key(Key::Up) => Action::SelectUp,
            Event::Key(Key::Down) => Action::SelectDown,
            _ => Action::None,
        }
    }
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
        Key::End => "⇲".to_string(),
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

fn menu_paragraph(entries: &[(String, &'static str)]) -> Paragraph<'static> {
    let mut spans = Vec::new();
    let mut sep = "";
    entries.iter().for_each(|(key, action)| {
        spans.push(Span::raw(sep));
        spans.push(Span::styled(
            key.to_string(),
            Style::default().add_modifier(Modifier::REVERSED),
        ));
        spans.push(Span::raw(format!(" {action}")));
        sep = "  ";
    });
    Paragraph::new(Line::from(spans)).alignment(Alignment::Left)
}

/// Navigation arrows dependending on table overflows
fn navigation_arrows(
    screen: Rect,
    offset: AreaProperty<usize>,
    table_width: u16,
    table_height: u16,
    first_col_width: usize,
) -> (Text<'static>, AreaProperty<bool>) {
    let (inner_width, inner_height) = (screen.width - 2, screen.height - 3);
    let up_arrow = if offset.vertical > 0 {
        "  ⬆  "
    } else {
        "   "
    };
    let voverflow = table_height > inner_height;
    let down_arrow = if voverflow { "⬇" } else { " " };
    let left_arrow = if offset.horizontal > 0 { "⬅" } else { " " };
    let hoverflow = table_width > inner_width;
    let right_arrow = if hoverflow { "➡" } else { " " };
    let mut nav = Text::from(format!("{up_arrow:^first_col_width$}"));
    nav.extend(Text::from(format!(
        "{:^first_col_width$}",
        format!("{left_arrow} {down_arrow} {right_arrow}",)
    )));
    (nav, AreaProperty::new(hoverflow, voverflow))
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
fn width_constraints(
    screen_width: u16,
    column_widths: &[u16],
    column_spacing: u16,
) -> (u16, Vec<Constraint>) {
    let mut total_width = 0;
    let mut constraints = Vec::with_capacity(column_widths.len());
    let mut current_column_spacing = 0;
    while constraints.len() < column_widths.len() {
        let index = constraints.len();
        let col_width = column_widths[index];
        let new_total_width = total_width + current_column_spacing + col_width;
        if new_total_width < screen_width {
            constraints.push(Constraint::Length(col_width));
        } else {
            let remaining = screen_width - total_width;
            if remaining > column_spacing {
                // Partial last column
                constraints.push(Constraint::Length(remaining - column_spacing));
                total_width = screen_width;
            }
            break;
        }
        total_width = new_total_width;
        current_column_spacing = column_spacing;
    }
    (total_width, constraints)
}

/// Compute the maximum length of strings
#[derive(Clone, Copy, Debug, Default)]
struct MaxLength(u16);

impl MaxLength {
    /// The length:
    fn len(&self) -> u16 {
        let Self(length) = self;
        *length
    }

    /// Count the maximun length of a string
    fn check(&mut self, s: &str) {
        let slen = s.len() as u16;
        if slen > self.0 {
            self.0 = slen
        }
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
    /// Horizontal and vertical offset
    table_offset: AreaProperty<usize>,
    /// Number of lines to scroll vertically up and down
    vertical_scroll: usize,
    /// Horizontal and vertical overflow (whether the table is bigger than the screen)
    overflow: AreaProperty<bool>,
    /// Column headers for metrics
    metric_headers: Vec<Text<'t>>,
    /// Slots where limits are displayed under the metric (only for raw metrics).
    limit_slots: Vec<bool>,
    /// Mode to display limits.
    display_limits: LimitKind,
    /// Display styles
    styles: Styles,
    /// Selected line in the table
    selected: usize,
}

impl<'t> TerminalDevice<'t> {
    pub fn new(every: Duration, theme: Option<BuiltinTheme>) -> anyhow::Result<Self> {
        let screen = io::stdout().into_raw_mode()?.into_alternate_screen()?;
        let backend = TermionBackend::new(Box::new(screen));
        let terminal = Terminal::new(backend)?;

        Ok(TerminalDevice {
            every,
            events: EventChannel::new(),
            terminal,
            table_offset: Default::default(),
            vertical_scroll: 1,
            overflow: AreaProperty::new(false, false),
            metric_headers: Vec::new(),
            limit_slots: Vec::new(),
            display_limits: LimitKind::None,
            styles: Styles::new(theme),
            selected: 0,
        })
    }

    pub fn is_available() -> bool {
        is_tty(&io::stdin())
    }

    /// Title of the outter box
    fn title(&self) -> String {
        let time_string = format!("{}", Local::now().format("%X"));
        let delay = human_duration(self.every);
        format!(" {time_string} / {delay} ")
    }

    /// Table headers and body
    ///
    /// Return (headers, rows, column_width)
    fn draw(
        &mut self,
        mut headers: Vec<Cell>,
        mut rows: Vec<Vec<Cell>>,
        nrows: usize,
        col_widths: &[u16],
    ) -> anyhow::Result<()> {
        let offset = self.table_offset;
        let title = self.title();
        let mut new_overflow = AreaProperty::new(false, false);
        let even_row_style = self.styles.even_row;
        let odd_row_style = self.styles.odd_row;
        let mut table_visible_height = 0;

        self.terminal.draw(|frame| {
            let screen = frame.area();
            let rects = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(screen.height - 1), Constraint::Min(0)].as_ref())
                .split(screen);
            let column_spacing = self.styles.column_spacing;
            let (table_width, widths) =
                width_constraints(screen.width, col_widths, self.styles.column_spacing);
            let table_height = 2u16 + nrows as u16;
            let (nav, overflow) = navigation_arrows(
                screen,
                offset,
                table_width,
                table_height,
                col_widths[0] as usize,
            );
            new_overflow = overflow;
            headers[0] = Cell::from(nav);

            const HEADERS_HEIGHT: u16 = 2;
            const BORDERS_HEIGHT: u16 = 2;
            let rows = style_rows(&mut rows, widths.len(), even_row_style, odd_row_style);
            let table = Table::new(rows, widths)
                .block(Block::default().borders(Borders::ALL).title(title))
                .header(Row::new(headers).height(HEADERS_HEIGHT))
                .column_spacing(column_spacing);
            frame.render_widget(table, rects[0]);

            const MENU_HEIGHT: u16 = 1;
            let display_limits = match self.display_limits {
                LimitKind::None => "Limit:Off",
                LimitKind::Soft => "Limit:Soft",
                LimitKind::Hard => "Limit:Hard",
            };
            let menu_entries = vec![
                (key_name(KEY_QUIT), "Quit"),
                (
                    format!("{}/{}", key_name(Key::Up), key_name(Key::Down)),
                    "Select",
                ),
                (format!("{KEY_FASTER_CHAR}/{KEY_SLOWER_CHAR}"), "Speed"),
                (key_name(KEY_LIMITS_UPPER), display_limits),
            ];
            let menu = menu_paragraph(&menu_entries);
            frame.render_widget(menu, rects[1]);
            table_visible_height = screen.height - HEADERS_HEIGHT - BORDERS_HEIGHT - MENU_HEIGHT;
        })?;
        self.overflow = new_overflow;
        self.vertical_scroll = if table_visible_height > 2 {
            table_visible_height / 2
        } else {
            1
        } as usize;
        Ok(())
    }

    /// Execute an interactive action.
    fn react(&mut self, action: Action, timer: &mut Timer) -> bool {
        const MAX_TIMEOUT_SECS: u64 = 24 * 3_600; // 24 hours
        const MIN_TIMEOUT_MSECS: u128 = 1;
        match action {
            Action::None => {}
            Action::Quit => return false,
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
                    self.table_offset.scroll_right(1)
                }
            }
            Action::ScrollUp => {
                self.table_offset.scroll_up(self.vertical_scroll);
            }
            Action::ScrollDown => {
                if self.overflow.vertical {
                    self.table_offset.scroll_down(self.vertical_scroll);
                }
            }
            Action::ScrollLeft => {
                self.table_offset.scroll_left(1);
            }
            Action::SelectUp => {
                self.selected = self.selected.saturating_sub(1);
            }
            Action::SelectDown => {
                self.selected += 1;
            }
        }
        true
    }

    /// Make the row of headers.
    ///
    /// The first column is the name. The second is the PID. The rest are the metrics.
    fn make_header_row<'p, 'w>(
        hoffset: usize,
        cws: &'w mut [MaxLength],
        metric_headers: Vec<Text<'p>>,
    ) -> Vec<Cell<'p>> {
        let column_count = cws.len();

        let mut row = Vec::with_capacity(column_count);
        row.push(Cell::from(""));
        const PID_TITLE: &'static str = "PID";
        row.push(Cell::from(
            Text::from(PID_TITLE).alignment(Alignment::Center),
        ));
        cws[1].check(PID_TITLE);

        metric_headers
            .iter()
            .skip(hoffset)
            .enumerate()
            .for_each(|(index, text)| {
                text.iter()
                    .for_each(|line| line.iter().for_each(|span| cws[index].check(&span.content)));
                row.push(Cell::from(text.clone().alignment(Alignment::Center)));
            });
        row
    }

    /// Make a row of metrics.
    ///
    /// The first column is the name. The second is the PID. The rest are the metrics.
    fn make_metrics_row<'p, 'w>(
        is_selected: bool,
        hoffset: usize,
        indent: usize,
        cws: &'w mut [MaxLength],
        ps: &'p ProcessSamples,
        styles: &Styles,
    ) -> Vec<Cell<'p>> {
        let column_count = cws.len();
        let mut row = Vec::with_capacity(column_count);
        cws[0].check(ps.name());
        let name_style = styles.name_style(is_selected);
        let name = {
            let name = ps.name();
            format!("{:>width$}", name, width = indent + name.len())
        };
        row.push(Cell::from(name).style(name_style));
        let pid = format!("{}", ps.pid());
        cws[1].check(&pid);
        row.push(rcell!(pid));
        let mut sample_col = 2;
        ps.samples()
            .map(|sample| izip!(sample.strings(), sample.trends()))
            .flatten()
            .skip(hoffset)
            .for_each(|(value, trend)| {
                cws[sample_col].check(value);
                sample_col += 1;
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
    fn make_limits_row<'p, 'w>(
        hoffset: usize,
        cws: &'w mut [MaxLength],
        ps: &'p ProcessSamples,
        display_limits: LimitKind,
        limit_slots: &[bool],
    ) -> Vec<Cell<'p>> {
        let column_count = cws.len();
        let mut row = Vec::with_capacity(column_count);
        const LIMITS_TITLE: &str = "limits";
        cws[0].check(LIMITS_TITLE);
        row.push(rcell!(LIMITS_TITLE));
        row.push(Cell::new(""));
        let mut col_index = 0;
        const NOT_APPLICABLE: &'static str = "n/a";
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

    fn render(&mut self, collector: &Collector, _targets_updated: bool) -> anyhow::Result<()> {
        let (hoffset, voffset) = (self.table_offset.horizontal, self.table_offset.vertical);
        let line_count = collector.line_count();
        let ncols = self.metric_headers.len() + 2; // process name, PID, metric1, ...
        let nrows = line_count + 2; // metric title, metric subtitle, process1, ...
        let nvisible_rows = nrows - voffset;
        let nvisible_cols = ncols - hoffset;

        if self.selected >= line_count {
            self.selected = line_count.saturating_sub(1);
        }

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
                .for_each(|(line_number, samples)| {
                    pids.push(samples);
                    let is_selected = line_number == self.selected;
                    let row = TerminalDevice::make_metrics_row(
                        is_selected,
                        hoffset,
                        pids.len().saturating_sub(1),
                        &mut cws,
                        &samples,
                        &self.styles,
                    );
                    rows.push(row);
                    if is_selected && with_limits {
                        let row = TerminalDevice::make_limits_row(
                            hoffset,
                            &mut cws,
                            &samples,
                            display_limits.clone(),
                            &self.limit_slots,
                        );
                        rows.push(row);
                    }
                });
            rows
        };

        let col_widths = cws.iter().map(MaxLength::len).collect::<Vec<u16>>();
        self.draw(headers, rows, nvisible_rows, &col_widths)?;
        Ok(())
    }

    /// Wait for a user input or a timeout.
    fn pause(&mut self, timer: &mut Timer) -> anyhow::Result<PauseStatus> {
        if let Some(timeout) = timer.remaining() {
            if let Some(evt) = self.events.receive_timeout(timeout)? {
                let action = Action::from(evt);
                if !self.react(action, timer) {
                    Ok(PauseStatus::Quit)
                } else {
                    Ok(PauseStatus::Interrupted)
                }
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
        let (table_width, widths) = width_constraints(SCREEN_WIDTH, &column_widths, COLUMN_SPACING);
        const SPACED_COLUMN_WIDTH: u16 = COLUMN_WIDTH + COLUMN_SPACING;
        const EXPECTED_WIDTH: u16 = FIRST_COLUMN_WIDTH + (NCOLS as u16 - 1) * SPACED_COLUMN_WIDTH;
        assert_eq!(EXPECTED_WIDTH, table_width);
        assert_eq!(NCOLS, widths.len());
        assert_eq!(Constraint::Length(FIRST_COLUMN_WIDTH), widths[0]);
        assert_eq!(Constraint::Length(COLUMN_WIDTH), widths[1]);
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
        let (table_width, widths) = width_constraints(SCREEN_WIDTH, &column_widths, COLUMN_SPACING);
        const SPACED_COLUMN_WIDTH: u16 = COLUMN_WIDTH + COLUMN_SPACING;
        let expected_width: u16 =
            FIRST_COLUMN_WIDTH + (column_widths.len() as u16 - 1) * SPACED_COLUMN_WIDTH;
        const EXPECTED_NCOLS: usize = 4;
        assert_eq!(expected_width, table_width);
        assert_eq!(EXPECTED_NCOLS, widths.len());
        assert_eq!(Constraint::Length(FIRST_COLUMN_WIDTH), widths[0]);
        assert_eq!(Constraint::Length(COLUMN_WIDTH), widths[1]);
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
        let (table_width, widths) = width_constraints(SCREEN_WIDTH, &column_widths, COLUMN_SPACING);
        const EXPECTED_NCOLS: usize = 5;
        assert_eq!(SCREEN_WIDTH, table_width);
        assert_eq!(EXPECTED_NCOLS, widths.len());
        assert_eq!(Constraint::Length(FIRST_COLUMN_WIDTH), widths[0]);
        assert_eq!(Constraint::Length(COLUMN_WIDTH), widths[1]);
        assert_eq!(Constraint::Length(2), widths[4]);
    }
}
