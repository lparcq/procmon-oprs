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

use std::convert::AsRef;
use std::io::{Result, Write};
use std::iter::{IntoIterator, Iterator};

use super::{sizer::ColumnSizer, COLUMN_SEPARATOR_WIDTH};
use crate::console::{
    charset::{TableChar, TableCharSet},
    Origin, Screen, Size,
};

const STYLE_NORMAL: u8 = 0x00;
const STYLE_HIGHLIGHT: u8 = 0x01;

pub struct Cell<'a> {
    value: &'a str,
    style: u8,
}

impl<'a> Cell<'a> {
    pub fn new(value: &'a str) -> Cell<'a> {
        Cell {
            value,
            style: STYLE_NORMAL,
        }
    }

    pub fn with_highlight(value: &'a str) -> Cell<'a> {
        Cell {
            value,
            style: STYLE_HIGHLIGHT,
        }
    }
}

impl AsRef<str> for Cell<'_> {
    fn as_ref(&self) -> &str {
        self.value
    }
}

macro_rules! cell_has_style {
    ($cell:expr, $style:expr) => {
        $cell.style & $style == $style
    };
}

/// Crosstab widget
///
/// Table with horizontal header and vertical header
pub struct TableDrawer<'a, 'b> {
    charset: &'a TableCharSet,
    sizer: &'b ColumnSizer,
    screen_size: Size,
    offset: (usize, usize),
    visible_columns: usize,
    border_width: usize,
    hrule: (usize, String),
    vline: &'static str,
}

impl<'a, 'b> TableDrawer<'a, 'b> {
    pub fn new(
        charset: &'a TableCharSet,
        sizer: &'b ColumnSizer,
        screen_size: Size,
        offset: (usize, usize),
        visible_columns: usize,
        border_width: usize,
    ) -> TableDrawer<'a, 'b> {
        let vline: &'static str = charset.get(TableChar::Vertical);
        let max_col_width = sizer.iter().max().unwrap_or(&0);
        // string that stores the largest line
        let hrule = charset.horizontal_line(*max_col_width);
        TableDrawer {
            charset,
            sizer,
            screen_size,
            offset,
            visible_columns,
            border_width,
            hrule,
            vline,
        }
    }

    fn skip_columns(&self, x: u16, count: usize) -> u16 {
        let mut x = x;
        if count > 0 {
            x += (self.sizer.width_or_zero(0) + self.border_width) as u16;
            let (horizontal_offset, _) = self.offset;
            let start_index = horizontal_offset + 1;
            let end_index = start_index + count - 1;
            for index in start_index..end_index {
                x += (self.sizer.width_or_zero(index) + COLUMN_SEPARATOR_WIDTH) as u16;
            }
        }
        x
    }

    fn write_column_rule(&self, screen: &mut Screen, index: usize, separator: &str) -> Result<()> {
        let (hlen, hrule) = &self.hrule;
        let column_width = self.sizer.width_or_zero(index);
        let hrule_len = column_width * hlen;
        write!(screen, "{}{}", separator, &hrule[0..hrule_len])
    }

    fn horizontal_rule(
        &self,
        screen: &mut Screen,
        origin: Origin,
        start_col: usize,
        left: &'static str,
        middle: &'static str,
        right: &'static str,
    ) -> Result<()> {
        let (horizontal_offset, _) = self.offset;
        let mut start_col = start_col;
        let mut separator = left;
        if start_col == 0 {
            screen.origin(origin)?;
            self.write_column_rule(screen, 0, separator)?;
            start_col = 1;
            separator = middle;
        } else {
            let Origin(x, y) = origin;
            screen.goto(self.skip_columns(x, start_col), y)?;
        }
        start_col += horizontal_offset;
        let column_count = start_col + self.visible_columns;
        for index in start_col..column_count {
            self.write_column_rule(screen, index, separator)?;
            separator = middle;
        }
        write!(screen, "{}", right)
    }

    /// Top line of the table
    pub fn top_line(&self, screen: &mut Screen, pos: Origin) -> Result<()> {
        self.horizontal_rule(
            screen,
            pos,
            1,
            self.charset.get(TableChar::DownRight),
            self.charset.get(TableChar::DownHorizontal),
            self.charset.get(TableChar::DownLeft),
        )
    }

    /// Line between the header and the body
    pub fn middle_line(&self, screen: &mut Screen, pos: Origin) -> Result<()> {
        self.horizontal_rule(
            screen,
            pos,
            0,
            self.charset.get(TableChar::DownRight),
            self.charset.get(TableChar::VerticalHorizontal),
            self.charset.get(TableChar::VerticalLeft),
        )
    }

    /// Top line of the table
    pub fn bottom_line(&self, screen: &mut Screen, pos: Origin) -> Result<()> {
        self.horizontal_rule(
            screen,
            pos,
            0,
            self.charset.get(TableChar::UpRight),
            self.charset.get(TableChar::UpHorizontal),
            self.charset.get(TableChar::UpLeft),
        )
    }

    pub fn write_horizontal_header<I, S>(
        &self,
        screen: &mut Screen,
        pos: Origin,
        row: I,
        bold: bool,
    ) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let (horizontal_offset, _) = self.offset;
        let offset = horizontal_offset + 1;
        let Origin(x, y) = pos;
        screen.goto(self.skip_columns(x, 1), y)?;
        for (index, value) in row.into_iter().take(self.visible_columns).enumerate() {
            let width = self.sizer.width_or_zero(index + offset);
            write!(screen, "{}", self.vline)?;
            if bold {
                screen.bold()?;
            }
            write!(screen, "{:^width$}", value.as_ref(), width = width)?;
            if bold {
                screen.style_reset()?;
            }
        }
        write!(screen, "{}", self.vline)
    }

    fn write_column<F>(
        &self,
        screen: &mut Screen,
        pos: Origin,
        index: usize,
        column: &[Cell<'_>],
        write_value: F,
    ) -> Result<()>
    where
        F: Fn(&mut Screen, &str, usize) -> Result<()>,
    {
        let (horizontal_offset, vertical_offset) = self.offset;
        let Size(_, screen_height) = self.screen_size;
        let Origin(x, mut y) = pos;
        let right = if index + 1 >= self.visible_columns {
            self.vline
        } else {
            ""
        };
        let width = self.sizer.width_or_zero(index + horizontal_offset);
        for cell in column.iter().skip(vertical_offset) {
            if y > screen_height {
                break;
            }
            screen.goto(x, y)?;
            let shade = (y % 2) == 0;
            write!(screen, "{}", self.vline)?;
            if shade {
                screen.shade(true)?;
            }
            if cell_has_style!(cell, STYLE_HIGHLIGHT) {
                screen.highlight(true)?;
            }
            write_value(screen, cell.value, width)?;
            if cell_has_style!(cell, STYLE_HIGHLIGHT) {
                screen.highlight(false)?;
            }
            if shade {
                screen.shade(false)?;
            }
            write!(screen, "{}", right)?;
            y += 1;
        }
        Ok(())
    }

    pub fn write_left_column(
        &self,
        screen: &mut Screen,
        pos: Origin,
        column: &[Cell<'_>],
    ) -> Result<()> {
        fn write_value(screen: &mut Screen, value: &str, width: usize) -> Result<()> {
            write!(screen, "{:<width$}", value, width = width)
        }
        self.write_column(screen, pos, 0, column, write_value)
    }

    pub fn write_middle_column(
        &self,
        screen: &mut Screen,
        pos: Origin,
        index: usize,
        column: &[Cell<'_>],
    ) -> Result<()> {
        fn write_value(screen: &mut Screen, value: &str, width: usize) -> Result<()> {
            write!(screen, "{:>width$}", value, width = width)
        }
        let pos = pos.with_x(self.skip_columns(pos.get_x(), index));
        self.write_column(screen, pos, index, column, write_value)
    }
}
