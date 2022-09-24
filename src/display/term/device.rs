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
    widgets::{Block, Borders, Cell, Row, Table},
    Terminal,
};

use crate::{
    agg::Aggregation,
    clock::Timer,
    collector::Collector,
    console::{is_tty, Event, EventChannel, Key},
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
        Key::Left => "⇲".to_string(),
        Key::Right => "⬅".to_string(),
        Key::Up => "⬆".to_string(),
        Key::Down => "⬇".to_string(),
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
    metric_names: Vec<String>,
    menu: tui::widgets::Paragraph<'static>,
}

impl TerminalDevice {
    pub fn new(every: Duration) -> anyhow::Result<TerminalDevice> {
        let screen = AlternateScreen::from(io::stdout().into_raw_mode()?);
        let backend = TermionBackend::new(Box::new(screen));
        let terminal = Terminal::new(backend)?;
        let mut sep = "";
        let mut spans = Vec::new();
        vec![
            (key_name(Key::Esc), "Quit"),
            (key_name(Key::PageUp), "Faster"),
            (key_name(Key::PageDown), "Slower"),
        ]
        .iter()
        .for_each(|(key, action)| {
            spans.push(Span::raw(sep));
            spans.push(Span::styled(
                key.to_string(),
                Style::default().add_modifier(Modifier::REVERSED),
            ));
            spans.push(Span::raw(format!(" {}", action)));
            sep = "  ";
        });
        let menu = tui::widgets::Paragraph::new(Spans::from(spans)).alignment(Alignment::Left);

        Ok(TerminalDevice {
            every,
            events: EventChannel::new(),
            terminal,
            table_offset: (0, 0),
            metric_names: Vec::new(),
            menu,
        })
    }

    pub fn is_available() -> bool {
        is_tty(&io::stdin())
    }

    /// Execute an interactive action.
    fn react(&mut self, action: Action, timer: &mut Timer) -> bool {
        const MAX_TIMEOUT_SECS: u64 = 24 * 3_600; // 24 hours
        const MIN_TIMEOUT_MSECS: u128 = 1;
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
                let (horizontal_offset, vertical_offset) = self.table_offset;
                self.table_offset = (horizontal_offset + 1, vertical_offset);
            }
            Action::ScrollUp => {
                let (horizontal_offset, vertical_offset) = self.table_offset;
                if vertical_offset > 0 {
                    self.table_offset = (horizontal_offset, vertical_offset - 1);
                }
            }
            Action::ScrollDown => {
                let (horizontal_offset, vertical_offset) = self.table_offset;
                self.table_offset = (horizontal_offset, vertical_offset + 1);
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
        collector.for_each_computed_metric(|id, ag| {
            if last_id.is_none() || last_id.unwrap() != id {
                last_id = Some(id);
                self.metric_names.push(id.as_str().to_string());
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
        Ok(())
    }

    /// Show the cursor on exit.
    fn close(&mut self) -> anyhow::Result<()> {
        self.terminal.show_cursor()?;
        Ok(())
    }

    fn render(&mut self, collector: &Collector, _targets_updated: bool) -> anyhow::Result<()> {
        let nrows = self.metric_names.len();
        let ncols = collector.len() + 1;
        let mut cw = MaxLength::new();

        let time_string = format!("{}", Local::now().format("%X"));
        let delay = human_duration(self.every);
        let title = format!(" {} / {} ", time_string, delay);

        let mut titles = Vec::with_capacity(ncols);
        titles.push(Cell::from(""));
        let mut rows: Vec<Vec<Cell>> = Vec::with_capacity(nrows);
        self.metric_names.iter().for_each(|name| {
            let mut row = Vec::with_capacity(ncols);
            row.push(Cell::from(cw.as_str(name.as_str())));
            rows.push(row);
        });
        // Pre-calculate column width
        collector.lines().for_each(|target| {
            cw.check(target.name());
            target.samples().for_each(|sample| {
                cw.iterate(sample.strings());
            });
        });
        collector.lines().for_each(|target| {
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
            titles.push(Cell::from(title));
            let mut row_index = 0;
            target.samples().for_each(|sample| {
                let changed = sample.changed();
                sample.strings().for_each(|value| {
                    rows[row_index].push(Cell::from(cw.right(value.as_str())).style(if changed {
                        Style::default().fg(Color::Blue)
                    } else {
                        Style::default()
                    }));
                    row_index += 1;
                })
            })
        });
        let widths = (0..ncols)
            .map(|_| Constraint::Length(cw.length as u16))
            .collect::<Vec<Constraint>>();
        let table = Table::new(rows.drain(..).map(|r| Row::new::<Vec<Cell>>(r)))
            .block(Block::default().borders(Borders::ALL).title(title))
            .header(Row::new(titles).height(2))
            .widths(&widths);
        let menu = self.menu.clone();
        self.terminal.draw(|f| {
            let screen = f.size();
            let rects = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(screen.height - 1), Constraint::Min(0)].as_ref())
                .split(screen);
            f.render_widget(table, rects[0]);
            f.render_widget(menu, rects[1]);
        })?;
        self.terminal.hide_cursor()?;
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
