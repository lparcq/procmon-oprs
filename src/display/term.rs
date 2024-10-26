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
    collector::{Collector, LimitKind},
    console::{is_tty, BuiltinTheme, Event, EventChannel, Key},
    display::{DisplayDevice, PauseStatus},
    format::human_duration,
};

/// Theme styles
struct Styles {
    even_row: Style,
    odd_row: Style,
    increase: Style,
    decrease: Style,
    column_spacing: u16,
}

impl Styles {
    fn new(theme: Option<BuiltinTheme>) -> Self {
        let default_style = Style::default();
        let (even_row, odd_row, increase, decrease, column_spacing) = match theme {
            Some(BuiltinTheme::Dark) => (
                default_style,
                Style::default().bg(Color::Rgb(40, 40, 40)),
                Style::default().fg(Color::Rgb(235, 45, 83)),
                Style::default().fg(Color::Rgb(166, 255, 77)),
                2,
            ),
            Some(BuiltinTheme::Light) => (
                default_style,
                Style::default().bg(Color::Rgb(215, 215, 215)),
                Style::default().fg(Color::Rgb(220, 20, 60)),
                Style::default().fg(Color::Rgb(102, 204, 00)),
                2,
            ),
            Some(BuiltinTheme::Dark16) => (
                default_style,
                Style::default().fg(Color::LightBlue),
                Style::default().fg(Color::LightMagenta),
                Style::default().fg(Color::LightGreen),
                2,
            ),
            Some(BuiltinTheme::Light16) => (
                default_style,
                Style::default().bg(Color::Blue),
                Style::default().fg(Color::Red),
                Style::default().fg(Color::Green),
                2,
            ),
            None => (
                default_style,
                default_style,
                Style::default().add_modifier(Modifier::BOLD),
                Style::default().add_modifier(Modifier::BOLD),
                2,
            ),
        };
        Styles {
            even_row,
            odd_row,
            increase,
            decrease,
            column_spacing,
        }
    }
}

/// Standard keys
const KEY_QUIT: Key = Key::Esc;
const KEY_FASTER: Key = Key::PageUp;
const KEY_SLOWER: Key = Key::PageDown;
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
            Event::Key(Key::Up) => Action::ScrollUp,
            Event::Key(Key::Down) => Action::ScrollDown,
            Event::Key(Key::Left) => Action::ScrollLeft,
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
        Key::Home => "⇱".to_string(),
        Key::End => "⇲".to_string(),
        KEY_FASTER => "PgUp".to_string(),
        KEY_SLOWER => "PgDn".to_string(),
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

fn menu_paragraph(entries: &[(Key, &'static str)]) -> Paragraph<'static> {
    let mut spans = Vec::new();
    let mut sep = "";
    entries.iter().for_each(|(key, action)| {
        spans.push(Span::raw(sep));
        spans.push(Span::styled(
            key_name(*key),
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
    hoffset: usize,
    voffset: usize,
    table_width: u16,
    table_height: u16,
    first_col_width: usize,
) -> (Text<'static>, bool, bool) {
    let (inner_width, inner_height) = (screen.width - 2, screen.height - 3);
    let up_arrow = if voffset > 0 { "  ⬆  " } else { "   " };
    let voverflow = table_height > inner_height;
    let down_arrow = if voverflow { "⬇" } else { " " };
    let left_arrow = if hoffset > 0 { "⬅" } else { " " };
    let hoverflow = table_width > inner_width;
    let right_arrow = if hoverflow { "➡" } else { " " };
    let mut nav = Text::from(format!("{up_arrow:^first_col_width$}"));
    nav.extend(Text::from(format!(
        "{:^first_col_width$}",
        format!("{left_arrow} {down_arrow} {right_arrow}",)
    )));
    (nav, voverflow, hoverflow)
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
            Row::new(r.drain(0..ncols)).style(style)
        })
        .collect::<Vec<Row>>()
}

/// Calculate widths constraints to avoid an overflow
fn width_constraints(
    screen_width: u16,
    column_widths: &[u16],
    default_column_width: u16,
    column_spacing: u16,
    ncols: usize,
) -> (u16, Vec<Constraint>) {
    let mut widths = Vec::with_capacity(ncols);
    let nfixed_cols = column_widths.len() as u16;
    let ndef_cols = ncols as u16 - nfixed_cols; // Number of columns with a default width.
    widths.extend_from_slice(column_widths);
    let fixed_width = column_widths.iter().sum::<u16>() + (column_spacing * (nfixed_cols - 1));
    let remaining: u16 = screen_width - fixed_width;
    let spaced_column_width = column_spacing + default_column_width;
    let nvisible_cols = std::cmp::min(remaining.saturating_div(spaced_column_width), ndef_cols);
    (0..nvisible_cols).for_each(|_| widths.push(default_column_width));
    let mut total_width = fixed_width + spaced_column_width * nvisible_cols;
    if nvisible_cols < ndef_cols {
        let remaining = remaining - nvisible_cols * spaced_column_width;
        if remaining > column_spacing {
            let last_col_width = remaining - column_spacing;
            widths.push(last_col_width);
            total_width += remaining;
        }
    }
    (
        total_width,
        widths
            .iter()
            .map(|w| Constraint::Length(*w))
            .collect::<Vec<Constraint>>(),
    )
}

/// Compute the maximum length of strings
struct MaxLength {
    length: usize,
}

impl MaxLength {
    fn new() -> MaxLength {
        MaxLength { length: 0 }
    }

    /// Count the maximun length of a string
    fn check(&mut self, s: &str) {
        if s.len() > self.length {
            self.length = s.len()
        }
    }

    /// Count the length and return the string.
    fn as_str<'a>(&mut self, s: &'a str) -> &'a str {
        self.check(s);
        s
    }
}

/// Print on standard output as a table
pub struct TerminalDevice {
    every: Duration,
    events: EventChannel,
    terminal: Terminal<TermionBackend<Box<AlternateScreen<RawTerminal<io::Stdout>>>>>,
    table_offset: (usize, usize),
    overflow: (bool, bool),
    metric_names: Vec<String>,
    metric_width: u16,
    display_limits: LimitKind,
    styles: Styles,
}

impl TerminalDevice {
    pub fn new(every: Duration, theme: Option<BuiltinTheme>) -> anyhow::Result<TerminalDevice> {
        let screen = io::stdout().into_raw_mode()?.into_alternate_screen()?;
        let backend = TermionBackend::new(Box::new(screen));
        let terminal = Terminal::new(backend)?;

        Ok(TerminalDevice {
            every,
            events: EventChannel::new(),
            terminal,
            table_offset: (0, 0),
            overflow: (false, false),
            metric_names: Vec::new(),
            metric_width: 0,
            display_limits: LimitKind::None,
            styles: Styles::new(theme),
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
        ncols: usize,
        col_widths: &[u16],
        default_col_width: u16,
    ) -> anyhow::Result<()> {
        let (hoffset, voffset) = self.table_offset;
        let title = self.title();
        let mut new_voverflow = false;
        let mut new_hoverflow = false;
        let even_row_style = self.styles.even_row;
        let odd_row_style = self.styles.odd_row;

        self.terminal.draw(|frame| {
            let screen = frame.area();
            let rects = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(screen.height - 1), Constraint::Min(0)].as_ref())
                .split(screen);
            if self.metric_width < screen.width {
                let column_spacing = self.styles.column_spacing;
                let (table_width, widths) = width_constraints(
                    screen.width,
                    col_widths,
                    default_col_width,
                    self.styles.column_spacing,
                    ncols,
                );
                let table_height: u16 = 2 + nrows as u16;
                let (nav, voverflow, hoverflow) = navigation_arrows(
                    screen,
                    hoffset,
                    voffset,
                    table_width,
                    table_height,
                    self.metric_width as usize,
                );
                new_voverflow = voverflow;
                new_hoverflow = hoverflow;
                headers[0] = Cell::from(nav);

                let rows = style_rows(&mut rows, widths.len(), even_row_style, odd_row_style);
                let table = Table::new(rows, widths)
                    .block(Block::default().borders(Borders::ALL).title(title))
                    .header(Row::new(headers).height(2))
                    .column_spacing(column_spacing);
                frame.render_widget(table, rects[0]);
            }

            let display_limits = match self.display_limits {
                LimitKind::None => "Limit:Off",
                LimitKind::Soft => "Limit:Soft",
                LimitKind::Hard => "Limit:Hard",
            };
            let menu_entries = vec![
                (KEY_QUIT, "Quit"),
                (KEY_FASTER, "Faster"),
                (KEY_SLOWER, "Slower"),
                (KEY_LIMITS_UPPER, display_limits),
            ];
            let menu = menu_paragraph(&menu_entries);
            frame.render_widget(menu, rects[1]);
        })?;
        self.overflow = (new_voverflow, new_hoverflow);
        Ok(())
    }

    /// Execute an interactive action.
    fn react(&mut self, action: Action, timer: &mut Timer) -> bool {
        const MAX_TIMEOUT_SECS: u64 = 24 * 3_600; // 24 hours
        const MIN_TIMEOUT_MSECS: u128 = 1;
        let (voverflow, hoverflow) = self.overflow;
        match action {
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
                if hoverflow {
                    let (horizontal_offset, vertical_offset) = self.table_offset;
                    self.table_offset = (horizontal_offset + 1, vertical_offset);
                }
            }
            Action::ScrollUp => {
                let (horizontal_offset, vertical_offset) = self.table_offset;
                if vertical_offset > 0 {
                    self.table_offset = (horizontal_offset, vertical_offset - 1);
                }
            }
            Action::ScrollDown => {
                if voverflow {
                    let (horizontal_offset, vertical_offset) = self.table_offset;
                    self.table_offset = (horizontal_offset, vertical_offset + 1);
                }
            }
            Action::ScrollLeft => {
                let (horizontal_offset, vertical_offset) = self.table_offset;
                if horizontal_offset > 0 {
                    self.table_offset = (horizontal_offset - 1, vertical_offset);
                }
            }
            _ => {}
        }
        true
    }
}

impl DisplayDevice for TerminalDevice {
    fn open(&mut self, collector: &Collector) -> anyhow::Result<()> {
        let mut last_id = None;
        let mut cw = MaxLength::new();
        collector.for_each_computed_metric(|id, ag| {
            if last_id.is_none() || last_id.unwrap() != id {
                last_id = Some(id);
                self.metric_names.push(cw.as_str(id.as_str()).to_string());
            } else {
                let name = format!(
                    "{} ({})",
                    id.as_str(),
                    match ag {
                        Aggregation::None => "none", // never used
                        Aggregation::Min => "min",
                        Aggregation::Max => "max",
                        Aggregation::Ratio => "%",
                    }
                );
                self.metric_names.push(name);
            }
        });
        self.metric_width = cw.length as u16;
        self.terminal.hide_cursor()?;
        Ok(())
    }

    /// Show the cursor on exit.
    fn close(&mut self) -> anyhow::Result<()> {
        self.terminal.show_cursor()?;
        Ok(())
    }

    fn render(&mut self, collector: &Collector, _targets_updated: bool) -> anyhow::Result<()> {
        let (hoffset, voffset) = self.table_offset;
        let ncols = collector.metric_count() + 2; // process name, PID, metric1, ...
        let nrows = collector.line_count() + 2; // metric title, metric subtitle, process1, ...
        let nvisible_rows = nrows - voffset;
        let nvisible_cols = ncols - hoffset;

        let bold_style = Style::default().add_modifier(Modifier::BOLD);
        let headers = {
            let mut headers = Vec::with_capacity(nvisible_cols);
            headers.push(Cell::from(""));
            headers.push(Cell::from(Text::from("PID").alignment(Alignment::Center)));
            self.metric_names.iter().skip(hoffset).for_each(|name| {
                headers.push(Cell::from(
                    Text::from(
                        name.split(':')
                            .map(|s| Line::from(s.to_string()).alignment(Alignment::Center))
                            .collect::<Vec<Line>>(),
                    )
                    .alignment(Alignment::Center),
                ))
            });
            headers
        };

        let mut cw_pid = MaxLength::new();
        let mut cw = MaxLength::new();
        let decrease_style = self.styles.decrease;
        let increase_style = self.styles.increase;
        let rows = {
            let mut rows: Vec<Vec<Cell>> = Vec::with_capacity(nvisible_rows);
            collector.lines().skip(voffset).for_each(|target| {
                let mut row = Vec::with_capacity(nvisible_cols);
                row.push(Cell::from(target.name()).style(bold_style));
                let pid = format!("{}", target.pid());
                cw_pid.check(&pid);
                row.push(Cell::from(Text::from(pid).alignment(Alignment::Right)));
                target.samples().skip(hoffset).for_each(|sample| {
                    izip!(sample.strings(), sample.trends()).for_each(|(value, trend)| {
                        cw.check(value);
                        row.push(Cell::from(
                            Text::from(value.as_str())
                                .style(match trend {
                                    Ordering::Less => decrease_style,
                                    Ordering::Equal => Style::default(),
                                    Ordering::Greater => increase_style,
                                })
                                .alignment(Alignment::Right),
                        ));
                    });
                });
                rows.push(row);
            });
            rows
        };

        let col_widths = vec![self.metric_width, cw_pid.length as u16];
        self.draw(
            headers,
            rows,
            nvisible_rows,
            nvisible_cols,
            &col_widths,
            cw.length as u16,
        )?;
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

    use super::{pivot_flatten, width_constraints, Collector, LimitKind};

    #[test]
    fn test_pivot_flatten() {
        let empty: &[Vec<Vec<&str>>] = &[];
        let collector0 = Collector::from(empty);
        let rows_with_limit = vec![false; 3];
        let values0 = pivot_flatten(&collector0, 0, 0, LimitKind::None, &rows_with_limit);
        assert_eq!(0, values0.len());

        let statuses1 = vec![
            vec![vec!["val111", "val112"], vec!["val12"]],
            vec![vec!["val211", "val212"], vec!["val22"]],
        ];
        let expected_values1 = vec![
            vec!["val111", "val211"],
            vec!["val112", "val212"],
            vec!["val12", "val22"],
        ];
        let collector1 = Collector::from(statuses1.as_slice());
        let values1 = pivot_flatten(&collector1, 3, 2, LimitKind::None, &rows_with_limit);
        assert_eq!(expected_values1.len(), values1.len());
        let values1 = values1
            .iter()
            .map(|row| row.iter().map(|(val, _trend)| *val).collect::<Vec<&str>>())
            .collect::<Vec<Vec<&str>>>();
        assert_eq!(expected_values1, values1);
    }

    #[test]
    fn test_width_constraints_underflow() {
        // 0         1
        // 01234567890123456789
        // aaaaa bbbb bbbb
        const SCREEN_WIDTH: u16 = 20;
        const FIRST_COLUMN_WIDTH: u16 = 5;
        const COLUMN_WIDTH: u16 = 4;
        const COLUMN_SPACING: u16 = 1;
        const NCOLS: usize = 3;
        let (table_width, widths) = width_constraints(
            SCREEN_WIDTH,
            FIRST_COLUMN_WIDTH,
            COLUMN_WIDTH,
            COLUMN_SPACING,
            NCOLS,
        );
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
        const COLUMN_SPACING: u16 = 1;
        const NCOLS: usize = 4;
        let (table_width, widths) = width_constraints(
            SCREEN_WIDTH,
            FIRST_COLUMN_WIDTH,
            COLUMN_WIDTH,
            COLUMN_SPACING,
            NCOLS,
        );
        const SPACED_COLUMN_WIDTH: u16 = COLUMN_WIDTH + COLUMN_SPACING;
        const EXPECTED_WIDTH: u16 = FIRST_COLUMN_WIDTH + (NCOLS as u16 - 1) * SPACED_COLUMN_WIDTH;
        assert_eq!(EXPECTED_WIDTH, table_width);
        assert_eq!(NCOLS, widths.len());
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
        const COLUMN_SPACING: u16 = 1;
        const NCOLS: usize = 6;
        let (table_width, widths) = width_constraints(
            SCREEN_WIDTH,
            FIRST_COLUMN_WIDTH,
            COLUMN_WIDTH,
            COLUMN_SPACING,
            NCOLS,
        );
        const EXPECTED_NCOLS: usize = 5;
        assert_eq!(SCREEN_WIDTH, table_width);
        assert_eq!(EXPECTED_NCOLS, widths.len());
        assert_eq!(Constraint::Length(FIRST_COLUMN_WIDTH), widths[0]);
        assert_eq!(Constraint::Length(COLUMN_WIDTH), widths[1]);
        assert_eq!(Constraint::Length(2), widths[4]);
    }
}
