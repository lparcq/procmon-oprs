// Oprs -- process monitor for Linux
// Copyright (C) 2020  Laurent Pelecq
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

use std::io::{self, Write};
use std::time::{Duration, Instant};
use termion::{
    clear,
    cursor::{self, Goto},
    input::MouseTerminal,
    raw::IntoRawMode,
    screen::AlternateScreen,
    terminal_size,
};

pub use self::charset::{TableChar, TableCharSet};

use self::{
    menu::{Action, MenuBar},
    sizer::ColumnSizer,
    table::TableDrawer,
};
use super::Output;
use crate::{agg::Aggregation, collector::Collector};

mod charset;
mod input;
mod menu;
mod sizer;
mod table;

const ELASTICITY: usize = 2;

/// Check if charset is unicode
pub fn is_unicode() -> bool {
    if let Ok(lang) = std::env::var("LANG") {
        match env_lang::to_struct(&lang) {
            Ok(lang) => {
                if let Some(charset) = lang.charset {
                    charset.to_lowercase().starts_with("utf")
                } else {
                    false
                }
            }
            _ => false,
        }
    } else {
        false
    }
}

pub type Size = (u16, u16);

pub trait Widget {
    fn write(&self, out: &mut dyn Write, pos: Goto, size: Size) -> io::Result<()>;
}

/// Print on standard output as a table
pub struct TerminalOutput {
    every: Duration,
    events: input::EventChannel,
    screen: Box<dyn Write>,
    menu: MenuBar,
    charset: TableCharSet,
    sizer: ColumnSizer,
    metric_names: Vec<String>,
}

impl TerminalOutput {
    pub fn new(every: Duration) -> anyhow::Result<TerminalOutput> {
        Ok(TerminalOutput {
            every,
            events: input::EventChannel::new(),
            screen: Box::new(AlternateScreen::from(MouseTerminal::from(
                io::stdout().into_raw_mode()?,
            ))),
            menu: MenuBar::new(),
            charset: TableCharSet::new(),
            sizer: ColumnSizer::new(ELASTICITY),
            metric_names: Vec::new(),
        })
    }

    pub fn is_available() -> bool {
        termion::is_tty(&io::stdin())
    }
}

impl Output for TerminalOutput {
    fn open(&mut self, collector: &Collector) -> anyhow::Result<()> {
        let mut last_id = None;
        collector.for_each_computed_metric(|id, ag| {
            if last_id.is_none() || last_id.unwrap() != id {
                last_id = Some(id);
                self.metric_names.push(id.to_str().to_string());
            } else {
                let name = format!(
                    "{} ({})",
                    id.to_str(),
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

    fn close(&mut self) -> anyhow::Result<()> {
        write!(self.screen, "{}", cursor::Show)?;
        self.screen.flush()?;
        Ok(())
    }

    fn render(&mut self, collector: &Collector, _targets_updated: bool) -> anyhow::Result<()> {
        // Prepare table
        self.sizer
            .overwrite(0, ColumnSizer::max_width(self.metric_names.as_slice()));
        let columns = collector
            .lines()
            .map(|proc| {
                proc.samples()
                    .map(|sample| sample.strings())
                    .flatten()
                    .map(|s| s.as_str())
                    .collect::<Vec<&str>>()
            })
            .collect::<Vec<Vec<&str>>>();
        columns.iter().enumerate().for_each(|(col_num, column)| {
            self.sizer
                .overwrite(col_num + 1, ColumnSizer::max_width(&column));
        });
        collector
            .lines()
            .map(|line| line.get_name().len())
            .enumerate()
            .for_each(|(index, len)| self.sizer.overwrite_min(index + 1, len));
        let subtitles = collector
            .lines()
            .map(|line| format!("{}", line.get_pid()))
            .collect::<Vec<String>>();
        subtitles
            .iter()
            .map(|s| s.len())
            .enumerate()
            .for_each(|(index, len)| self.sizer.overwrite_min(index + 1, len));

        self.sizer.truncate(columns.len() + 1);
        let _ = self.sizer.freeze();

        log::debug!("cols {:?}", self.sizer.iter().collect::<Vec<&usize>>());
        // Draw table
        let screen_size = terminal_size()?;
        let table = TableDrawer::new(&self.charset, &self.sizer, screen_size);
        let screen = &mut self.screen;
        write!(*screen, "{}", clear::All)?;
        table.top_line(screen, Goto(1, 1))?;
        table.write_horizontal_header1(
            screen,
            Goto(1, 2),
            collector.lines().map(|line| line.get_name()),
        )?;
        table.write_horizontal_header1(screen, Goto(1, 3), subtitles.iter())?;
        table.middle_line(screen, Goto(1, 4))?;
        let pos = Goto(1, 5);
        table.write_left_column(screen, pos, self.metric_names.iter())?;
        for (col_num, column) in columns.iter().enumerate() {
            table.write_middle_column(screen, pos, col_num + 1, column.iter())?;
        }
        table.bottom_line(screen, Goto(1, (self.metric_names.len() + 5) as u16))?;
        // Draw menu
        let (screen_width, screen_height) = screen_size;
        self.menu
            .write(&mut self.screen, Goto(1, screen_height), (screen_width, 1))?;
        write!(self.screen, "{}", cursor::Hide)?;
        self.screen.flush()?;
        Ok(())
    }

    fn pause(&mut self) -> anyhow::Result<bool> {
        let mut timeout = self.every;
        let stop_watch = Instant::now();
        while let Some(evt) = self.events.receive_timeout(timeout)? {
            match timeout.checked_sub(stop_watch.elapsed()) {
                Some(rest) => timeout = rest,
                None => timeout = self.every,
            }
            match self.menu.action(&evt) {
                Action::Quit => return Ok(false),
                Action::MultiplyTimeout(factor) => {
                    if let Some(every) = self.every.checked_mul(factor as u32) {
                        self.every = every;
                    }
                }
                Action::DivideTimeout(factor) => {
                    if let Some(every) = self.every.checked_div(factor as u32) {
                        self.every = every;
                    }
                }
                _ => {}
            }
        }
        Ok(true)
    }
}
