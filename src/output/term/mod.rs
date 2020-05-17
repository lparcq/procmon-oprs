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

use std::cmp::max;
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
use super::{Output, PauseStatus};
use crate::{agg::Aggregation, collector::Collector};

mod charset;
mod input;
mod menu;
mod sizer;
mod table;

const ELASTICITY: usize = 2;

const BORDER_WIDTH: usize = 1;
const MENU_HEIGHT: usize = 1;
const HEADER_HEIGHT: usize = 2;

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

pub type ScreenSize = (u16, u16);

pub trait Widget {
    fn write(&self, out: &mut dyn Write, pos: Goto, size: ScreenSize) -> io::Result<()>;
}

/// Print on standard output as a table
pub struct TerminalOutput {
    every: Duration,
    events: input::EventChannel,
    screen: Box<dyn Write>,
    charset: TableCharSet,
    menu: MenuBar,
    sizer: ColumnSizer,
    table_offset: (usize, usize),
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
            charset: TableCharSet::new(),
            menu: MenuBar::new(),
            sizer: ColumnSizer::new(ELASTICITY),
            table_offset: (0, 0),
            metric_names: Vec::new(),
        })
    }

    pub fn is_available() -> bool {
        termion::is_tty(&io::stdin())
    }

    fn header_side_symbol(&mut self, y: u16, symbol: &str) -> io::Result<()> {
        write!(
            self.screen,
            "{}{}",
            Goto(self.sizer.width_or_zero(0) as u16, y),
            symbol
        )
    }

    fn arrow_up(&mut self) -> io::Result<()> {
        self.header_side_symbol(2, self.charset.get(TableChar::ArrowUp))
    }

    fn arrow_down(&mut self) -> io::Result<()> {
        self.header_side_symbol(3, self.charset.get(TableChar::ArrowDown))
    }

    fn recenter_table(
        &mut self,
        _screen_width: usize,
        screen_height: usize,
        table_height: usize,
    ) -> bool {
        let (horizontal_offset, mut vertical_offset) = self.table_offset;
        if table_height < screen_height {
            vertical_offset = 0;
        } else if table_height - screen_height <= vertical_offset {
            vertical_offset = table_height - screen_height;
        }
        self.table_offset = (horizontal_offset, vertical_offset);
        vertical_offset > 0
    }

    /// Calculate the columns width
    fn prepare<I1, I2>(&mut self, title_widths: I1, subtitles: I2, columns: &[Vec<&str>])
    where
        I1: IntoIterator<Item = usize>,
        I2: IntoIterator<Item = usize>,
    {
        self.sizer
            .overwrite(0, ColumnSizer::max_width(self.metric_names.as_slice()));
        columns.iter().enumerate().for_each(|(col_num, column)| {
            self.sizer
                .overwrite(col_num + 1, ColumnSizer::max_width(&column));
        });

        title_widths
            .into_iter()
            .zip(subtitles.into_iter())
            .map(|(tlen, stlen)| max(tlen, stlen))
            .enumerate()
            .for_each(|(index, len)| self.sizer.overwrite_min(index + 1, len));
    }

    fn write_table<I1, I2, S>(
        &mut self,
        titles: I1,
        subtitles: I2,
        columns: &[Vec<S>],
        screen_size: ScreenSize,
        table_height: u16,
    ) -> io::Result<bool>
    where
        I1: IntoIterator<Item = S>,
        I2: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let (screen_width, screen_height) = screen_size;
        let table = TableDrawer::new(
            &self.charset,
            &self.sizer,
            (screen_width, screen_height),
            self.table_offset,
        );
        let screen = &mut self.screen;
        write!(*screen, "{}", clear::All)?;
        table.top_line(screen, Goto(1, 1))?;
        table.write_horizontal_header1(screen, Goto(1, 2), titles)?;
        table.write_horizontal_header1(screen, Goto(1, 3), subtitles)?;
        table.middle_line(screen, Goto(1, 4))?;
        let pos = Goto(1, 5);
        let eos_y = table.write_left_column(screen, pos, self.metric_names.iter())?;
        for (col_num, column) in columns.iter().enumerate() {
            table.write_middle_column(screen, pos, col_num + 1, column.iter())?;
        }
        let (_, vertical_offset) = self.table_offset;
        let bottom_y = table_height - (vertical_offset as u16);
        if bottom_y <= screen_height {
            table.bottom_line(screen, Goto(1, bottom_y))?;
        }
        Ok(eos_y)
    }

    fn react(&mut self, action: Action) -> bool {
        match action {
            Action::Quit => return false,
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
            _ => {}
        }
        true
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
        let subtitles = collector
            .lines()
            .map(|line| format!("{}", line.get_pid()))
            .collect::<Vec<String>>();
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
        // Prepare table
        self.prepare(
            collector.lines().map(|line| line.get_name().len()),
            subtitles.iter().map(|s| s.len()),
            &columns,
        );

        self.sizer.truncate(columns.len() + 1);
        let _ = self.sizer.freeze();

        // Draw table
        let table_height = self.metric_names.len() + HEADER_HEIGHT + 3 * BORDER_WIDTH;
        let (screen_width, screen_height) = terminal_size()?;
        let vscrolled = self.recenter_table(
            screen_width as usize,
            screen_height as usize - MENU_HEIGHT,
            table_height,
        );
        let eos_y = self.write_table(
            collector.lines().map(|line| line.get_name()),
            subtitles.iter().map(|s| s.as_str()),
            &columns,
            (screen_width, screen_height - (MENU_HEIGHT as u16)),
            table_height as u16,
        )?;
        if eos_y {
            self.arrow_down()?;
        }
        if vscrolled {
            self.arrow_up()?;
        }
        // Draw menu
        self.menu
            .write(&mut self.screen, Goto(1, screen_height), (screen_width, 1))?;
        write!(self.screen, "{}", cursor::Hide)?;
        self.screen.flush()?;
        Ok(())
    }

    fn pause(&mut self, remaining: Option<Duration>) -> anyhow::Result<PauseStatus> {
        let timeout = remaining.unwrap_or(self.every);
        let stop_watch = Instant::now();
        if let Some(evt) = self.events.receive_timeout(timeout)? {
            let action = self.menu.action(&evt);
            if !self.react(action) {
                return Ok(PauseStatus::Stop);
            }
            if let Some(remaining) = timeout.checked_sub(stop_watch.elapsed()) {
                return Ok(PauseStatus::Remaining(remaining));
            }
        }
        Ok(PauseStatus::TimeOut)
    }
}
