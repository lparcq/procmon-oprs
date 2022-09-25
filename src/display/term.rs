// Oprs -- process monitor for Linux
// Copyright (C) 2020, 2021  Laurent Pelecq
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
use std::io;
use std::time::Duration;
use termion::{
    raw::{IntoRawMode, RawTerminal},
    screen::AlternateScreen,
};
use tui::{
    backend::TermionBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans, Text},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Terminal,
};

use crate::{
    agg::Aggregation,
    clock::Timer,
    collector::Collector,
    console::{is_tty, BuiltinTheme, Event, EventChannel, Key},
    display::{DisplayDevice, PauseStatus},
    format::human_duration,
};

/// Action
pub enum Action {
    None,
    Quit,
    MultiplyTimeout(u16),
    DivideTimeout(u16),
    ScrollRight,
    ScrollUp,
    ScrollDown,
    ScrollLeft,
}

impl From<Event> for Action {
    fn from(evt: Event) -> Self {
        match evt {
            Event::Key(Key::Esc) => Action::Quit,
            Event::Key(Key::Ctrl('c')) => Action::Quit,
            Event::Key(Key::PageUp) => Action::DivideTimeout(2),
            Event::Key(Key::PageDown) => Action::MultiplyTimeout(2),
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
        Key::PageUp => "PgUp".to_string(),
        Key::PageDown => "PgDn".to_string(),
        Key::BackTab => "⇤".to_string(),
        Key::Delete => "⌧".to_string(),
        Key::Insert => "Ins".to_string(),
        Key::F(num) => format!("F{}", num),
        Key::Char('\t') => "⇥".to_string(),
        Key::Char(ch) => format!("{}", ch),
        Key::Alt(ch) => format!("M-{}", ch),
        Key::Ctrl(ch) => format!("C-{}", ch),
        Key::Null => "\\0".to_string(),
        Key::Esc => "Esc".to_string(),
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
        spans.push(Span::raw(format!(" {}", action)));
        sep = "  ";
    });
    tui::widgets::Paragraph::new(Spans::from(spans)).alignment(Alignment::Left)
}

/// Change a list of target samples (columns) into rows and flatten the samples.
fn pivot_flatten<'a>(collector: &'a Collector, nrows: usize, ncols: usize) -> Vec<Vec<&'a str>> {
    let mut values = Vec::with_capacity(nrows);
    for _ in 0..nrows {
        values.push(Vec::with_capacity(ncols));
    }
    collector.lines().for_each(|target| {
        let mut row_index = 0;
        target.samples().for_each(|sample| {
            sample.strings().for_each(|value| {
                values[row_index].push(value.as_str());
                row_index += 1;
            });
        });
    });
    values
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

    /// Count the maximun length of strings
    fn iterate<I, S>(&mut self, it: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for s in it.into_iter() {
            self.check(s.as_ref());
        }
    }

    /// Count the length and return the string.
    fn as_str<'a>(&mut self, s: &'a str) -> &'a str {
        self.check(s);
        s
    }

    /// Return the string centered
    fn center(&self, s: &str) -> String {
        format!("{:^width$}", s, width = self.length)
    }

    /// Return the string centered
    fn right(&self, s: &str) -> String {
        format!("{:>width$}", s, width = self.length)
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
    theme: Option<BuiltinTheme>,
    column_spacing: u16,
}

impl TerminalDevice {
    pub fn new(every: Duration, theme: Option<BuiltinTheme>) -> anyhow::Result<TerminalDevice> {
        let screen = AlternateScreen::from(io::stdout().into_raw_mode()?);
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
            theme,
            column_spacing: 2,
        })
    }

    pub fn is_available() -> bool {
        is_tty(&io::stdin())
    }

    /// Title of the outter box
    fn title(&self) -> String {
        let time_string = format!("{}", Local::now().format("%X"));
        let delay = human_duration(self.every);
        format!(" {} / {} ", time_string, delay)
    }

    /// Navigation arrows
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
        let mut nav = Text::from(format!("{:^width$}", up_arrow, width = first_col_width));
        nav.extend(Text::from(format!(
            "{:^width$}",
            format!("{} {} {}", left_arrow, down_arrow, right_arrow),
            width = first_col_width
        )));
        (nav, voverflow, hoverflow)
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
        col_width: u16,
    ) -> anyhow::Result<()> {
        let (hoffset, voffset) = self.table_offset;
        let mut widths = Vec::with_capacity(ncols);
        let column_spacing = self.column_spacing;
        widths.push(self.metric_width);
        (0..ncols).for_each(|_| widths.push(col_width));
        let table_width: u16 =
            widths.iter().sum::<u16>() + ((widths.len() - 1) as u16) * column_spacing;
        let table_height: u16 = 2 + nrows as u16;
        let widths = widths
            .iter()
            .map(|w| Constraint::Length(*w))
            .collect::<Vec<Constraint>>();
        let title = self.title();
        let first_col_width = self.metric_width as usize;
        let mut new_voverflow = false;
        let mut new_hoverflow = false;
        let theme = self.theme;
        let rows = rows.drain(..).enumerate().map(|(i, r)| {
            let style = if i % 2 != 0 {
                Style::default()
            } else {
                match theme {
                    None => Style::default(),
                    Some(BuiltinTheme::Dark) => Style::default().bg(Color::Rgb(40, 40, 40)),
                    Some(BuiltinTheme::Light) => Style::default().bg(Color::Rgb(215, 215, 215)),
                }
            };
            Row::new::<Vec<Cell>>(r).style(style)
        });

        self.terminal.draw(|frame| {
            let screen = frame.size();
            let (nav, voverflow, hoverflow) = TerminalDevice::navigation_arrows(
                screen,
                hoffset,
                voffset,
                table_width,
                table_height,
                first_col_width,
            );
            new_voverflow = voverflow;
            new_hoverflow = hoverflow;
            headers[0] = Cell::from(nav);

            let table = Table::new(rows)
                .block(Block::default().borders(Borders::ALL).title(title))
                .header(Row::new(headers).height(2))
                .widths(&widths)
                .column_spacing(column_spacing);
            let rects = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(screen.height - 1), Constraint::Min(0)].as_ref())
                .split(screen);
            frame.render_widget(table, rects[0]);

            let menu_entries = vec![
                (Key::Esc, "Quit"),
                (Key::PageUp, "Faster"),
                (Key::PageDown, "Slower"),
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
        let nrows = self.metric_names.len();
        let ncols = collector.len() + 1;
        let nvisible_rows = nrows - voffset;
        let nvisible_cols = ncols - hoffset;

        let mut headers = Vec::with_capacity(nvisible_cols);
        headers.push(Cell::from(""));

        let mut rows: Vec<Vec<Cell>> = Vec::with_capacity(nvisible_rows);
        self.metric_names.iter().skip(voffset).for_each(|name| {
            let mut row = Vec::with_capacity(nvisible_cols);
            row.push(Cell::from(name.to_string()));
            rows.push(row);
        });

        let mut cw = MaxLength::new();

        let values = pivot_flatten(collector, nrows, ncols);
        values.iter().skip(hoffset).for_each(|row| cw.iterate(row));
        collector.lines().skip(hoffset).for_each(|target| {
            cw.check(target.name());
        });
        collector.lines().skip(hoffset).for_each(|target| {
            let mut title = Text::styled(
                cw.center(target.name()),
                Style::default().add_modifier(Modifier::BOLD),
            );
            let subtitle = match target.count() {
                Some(count) => format!("({})", count),
                None => format!("{}", target.pid()),
            };
            cw.check(&subtitle);
            title.extend(Text::from(cw.center(&subtitle)));
            headers.push(Cell::from(title));
        });
        values
            .iter()
            .skip(voffset)
            .enumerate()
            .for_each(|(row_index, samples)| {
                rows[row_index].extend(
                    samples
                        .iter()
                        .skip(hoffset)
                        .map(|sample| Cell::from(cw.right(*sample))),
                );
            });
        self.draw(
            headers,
            rows,
            nvisible_rows,
            nvisible_cols,
            cw.length as u16,
        )?;
        Ok(())
    }

    /// Terminal is interactive
    fn is_interactive(&self) -> bool {
        true
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

    use super::{pivot_flatten, Collector};

    #[test]
    fn test_pivot_flatten() {
        let empty: &[Vec<Vec<&str>>] = &[];
        let collector0 = Collector::from(empty);
        let values0 = pivot_flatten(&collector0, 0, 0);
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
        let values1 = pivot_flatten(&collector1, 3, 2);
        assert_eq!(expected_values1.len(), values1.len());
        assert_eq!(expected_values1, values1);
    }
}
