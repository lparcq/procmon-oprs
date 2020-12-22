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

use chrono::Local;
use std::io::{self, Write};
use std::time::Duration;

use super::{
    menu::{Action, MenuBar},
    sizer::ColumnSizer,
    table::{Cell, TableDrawer},
    Widget, BORDER_WIDTH, ELASTICITY, HEADER_HEIGHT, MENU_HEIGHT,
};
use crate::{
    agg::Aggregation,
    clock::Timer,
    collector::Collector,
    console::{
        charset::{TableChar, TableCharSet},
        is_tty, Clip, EventChannel, Origin, Screen, Size,
    },
    display::{DisplayDevice, PauseStatus},
    format::human_duration,
};

/// Print on standard output as a table
pub struct TerminalDevice {
    every: Duration,
    events: EventChannel,
    screen: Screen,
    charset: TableCharSet,
    menu: MenuBar,
    sizer: ColumnSizer,
    table_offset: (usize, usize),
    metric_names: Vec<String>,
}

impl TerminalDevice {
    pub fn new(every: Duration, screen: Screen) -> anyhow::Result<TerminalDevice> {
        Ok(TerminalDevice {
            every,
            events: EventChannel::new(),
            screen,
            charset: TableCharSet::new(),
            menu: MenuBar::new(),
            sizer: ColumnSizer::new(ELASTICITY),
            table_offset: (0, 0),
            metric_names: Vec::new(),
        })
    }

    pub fn is_available() -> bool {
        is_tty(&io::stdin())
    }

    /// Calculate the number of visible columns without counting the left header.
    /// Return also the number of columns that could be visible if there were
    /// scrolled to the left.
    fn number_of_visible_columns(&self, screen_width: u16) -> (usize, usize) {
        let mut visible_columns = 0;
        let (horizontal_offset, _) = self.table_offset;
        let mut width = self.sizer.width_or_zero(0) + BORDER_WIDTH; // left header
        let start_index = horizontal_offset + 1;
        for index in start_index..self.sizer.len() {
            width += self.sizer.width_or_zero(index) + BORDER_WIDTH;
            if width >= screen_width as usize {
                break;
            }
            visible_columns += 1;
        }
        let mut scrollable_columns = 0;
        for offset in 0..start_index {
            width += self.sizer.width_or_zero(start_index - offset) + BORDER_WIDTH;
            if width >= screen_width as usize {
                break;
            }
            scrollable_columns += 1; // column could be visible
        }
        (visible_columns, scrollable_columns)
    }

    /// Scroll table left or up to fill available space.
    /// Return if the table is scrollable on left, up, down, right
    fn recenter_table(
        &mut self,
        screen_width: usize,
        screen_height: usize,
        table_height: usize,
    ) -> (bool, bool, bool, bool) {
        let (mut horizontal_offset, mut vertical_offset) = self.table_offset;
        if table_height < screen_height {
            vertical_offset = 0;
        } else if table_height - screen_height <= vertical_offset {
            vertical_offset = table_height - screen_height;
        }
        let (mut visible_columns, scrollable_columns) =
            self.number_of_visible_columns(screen_width as u16);
        if horizontal_offset >= scrollable_columns {
            horizontal_offset -= scrollable_columns;
            visible_columns += scrollable_columns;
        }
        self.table_offset = (horizontal_offset, vertical_offset);
        let left_scrollable = horizontal_offset > 0;
        let up_scrollable = vertical_offset > 0;
        let down_scrollable = table_height - vertical_offset > screen_height;
        let right_scrollable = (self.sizer.len() - 1) - horizontal_offset > visible_columns;
        (
            left_scrollable,
            up_scrollable,
            down_scrollable,
            right_scrollable,
        )
    }

    /// Calculate the columns width
    ///
    /// `title_widths` contains the width of the static titles, `subtitles` and `columns`
    // contain the values of the rest of the table.
    fn prepare<I1, I2>(&mut self, title_widths: I1, subtitle_widths: I2, columns: &[Vec<Cell>])
    where
        I1: IntoIterator<Item = usize>,
        I2: IntoIterator<Item = usize>,
    {
        self.sizer.overwrite(
            0,
            ColumnSizer::strings_max_width(self.metric_names.as_slice()),
        );
        columns.iter().enumerate().for_each(|(col_num, column)| {
            self.sizer
                .overwrite(col_num + 1, ColumnSizer::strings_max_width(&column));
        });

        // Overwrite column widths with the respective size of titles and subtitles
        self.sizer.overwrite_mins(1, title_widths);
        self.sizer.overwrite_mins(1, subtitle_widths);
    }

    // Set column of equal size if it fits on screen.
    fn equalize_columns(&mut self, screen_width: usize) {
        let max_column_width = self.sizer.max_width_after(1); // max width not including left header
        let number_of_columns = self.sizer.len();
        let best_table_width = self.sizer.width_or_zero(0)
            + max_column_width * (number_of_columns - 1)
            + BORDER_WIDTH * (number_of_columns + 1);
        if best_table_width < screen_width {
            self.sizer.overwrite_mins_equally(1, max_column_width);
        }
    }

    /// Write the visible part of the table
    fn write_table<'a, I1, I2, S>(
        &mut self,
        titles: I1,
        subtitles: I2,
        columns: &[Vec<Cell<'a>>],
        screen_size: Size,
        table_height: u16,
    ) -> io::Result<()>
    where
        I1: IntoIterator<Item = S>,
        I2: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let Size(screen_width, screen_height) = screen_size;
        let (visible_columns, _) = self.number_of_visible_columns(screen_width);
        let table = TableDrawer::new(
            &self.charset,
            &self.sizer,
            screen_size,
            self.table_offset,
            visible_columns,
        );
        let (horizontal_offset, vertical_offset) = self.table_offset;
        let screen = &mut self.screen;
        table.top_line(screen, Origin(1, 1))?;
        table.write_horizontal_header(
            screen,
            Origin(1, 2),
            titles.into_iter().skip(horizontal_offset),
            true,
        )?;
        table.write_horizontal_header(
            screen,
            Origin(1, 3),
            subtitles.into_iter().skip(horizontal_offset),
            false,
        )?;
        table.middle_line(screen, Origin(1, 4))?;
        let pos = Origin(1, 5);
        let left_cells = self
            .metric_names
            .iter()
            .map(|value| Cell::new(value))
            .collect::<Vec<Cell>>();
        table.write_left_column(screen, pos, &left_cells)?;
        for (col_num, column) in columns
            .iter()
            .skip(horizontal_offset)
            .take(visible_columns)
            .enumerate()
        {
            table.write_middle_column(screen, pos, col_num + 1, column)?;
        }
        let bottom_y = table_height - (vertical_offset as u16);
        if bottom_y <= screen_height {
            table.bottom_line(screen, Origin(1, bottom_y))?;
        }
        Ok(())
    }

    /// Write a symbol in a cross in the left top part
    fn header_cross_symbol(&mut self, dx: u16, dy: u16, symbol: &str) -> io::Result<()> {
        let x = (self.sizer.width_or_zero(0) as u16) - dx - 1;
        let y = 3 - dy;
        self.screen.goto(x, y)?;
        write!(self.screen, "{}", symbol)
    }

    /// Write arrows according of the part of the table that can be scrolled.
    fn write_arrows(&mut self, scrollable: (bool, bool, bool, bool)) -> io::Result<()> {
        let (left, up, down, right) = scrollable;
        if left {
            self.header_cross_symbol(2, 1, self.charset.get(TableChar::ArrowLeft))?;
        }
        if up {
            self.header_cross_symbol(1, 2, self.charset.get(TableChar::ArrowUp))?;
        }
        if down {
            self.header_cross_symbol(1, 0, self.charset.get(TableChar::ArrowDown))?;
        }
        if right {
            self.header_cross_symbol(0, 1, self.charset.get(TableChar::ArrowRight))?;
        }
        Ok(())
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
        self.screen.cursor_show()?.flush()?;
        Ok(())
    }

    fn render(&mut self, collector: &Collector, _targets_updated: bool) -> anyhow::Result<()> {
        let Size(screen_width, screen_height) = self.screen.size()?;
        let subtitles = collector
            .lines()
            .map(|line| match line.count() {
                Some(count) => format!("({})", count),
                None => format!("{}", line.pid()),
            })
            .collect::<Vec<String>>();
        let columns = collector
            .lines()
            .map(|pstat| {
                pstat
                    .samples()
                    .map(|sample| {
                        let changed = sample.changed();
                        sample.strings().map(move |s| {
                            if changed {
                                Cell::with_highlight(s.as_str())
                            } else {
                                Cell::new(s.as_str())
                            }
                        })
                    })
                    .flatten()
                    .collect::<Vec<Cell>>()
            })
            .collect::<Vec<Vec<Cell>>>();
        // Prepare table
        self.prepare(
            collector.lines().map(|line| line.name().len()),
            subtitles.iter().map(|s| s.len()),
            &columns,
        );
        self.sizer.truncate(columns.len() + 1);
        self.equalize_columns(screen_width as usize);
        let _ = self.sizer.freeze();

        let now = Local::now().format("%X").to_string();
        self.screen.clear_all()?.goto(2, 2)?;
        write!(self.screen, "{}", now)?;
        self.screen.goto(2, 3)?;
        write!(self.screen, "{}", human_duration(self.every))?;

        // Draw table
        let table_height = self.metric_names.len() + HEADER_HEIGHT + 3 * BORDER_WIDTH;
        let scrollable = self.recenter_table(
            screen_width as usize,
            screen_height as usize - MENU_HEIGHT,
            table_height,
        );
        self.write_table(
            collector.lines().map(|line| line.name()),
            subtitles.iter().map(|s| s.as_str()),
            &columns,
            Size(screen_width, screen_height - (MENU_HEIGHT as u16)),
            table_height as u16,
        )?;
        self.write_arrows(scrollable)?;
        // Draw menu
        self.menu
            .write(&mut self.screen, &Clip(1, screen_height, screen_width, 1))?;
        self.screen.cursor_hide()?.flush()?;
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
                let action = self.menu.action(&evt);
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
