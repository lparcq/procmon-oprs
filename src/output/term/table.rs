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

use std::io::{Result, Write};
use std::iter::{IntoIterator, Iterator};
use termion::cursor::Goto;

use super::{
    charset::{TableChar, TableCharSet},
    sizer::ColumnSizer,
    ScreenSize, BORDER_WIDTH,
};

/// Crosstab widget
///
/// Table with horizontal header and vertical header
pub struct TableDrawer<'a, 'b> {
    charset: &'a TableCharSet,
    sizer: &'b ColumnSizer,
    screen_size: ScreenSize,
    offset: (usize, usize),
    visible_columns: usize,
    hrule: (usize, String),
    vline: &'static str,
}

impl<'a, 'b> TableDrawer<'a, 'b> {
    pub fn new(
        charset: &'a TableCharSet,
        sizer: &'b ColumnSizer,
        screen_size: ScreenSize,
        offset: (usize, usize),
        visible_columns: usize,
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
            hrule,
            vline,
        }
    }

    fn skip_columns(&self, pos: Goto, count: usize) -> Goto {
        let Goto(mut x, y) = pos;
        if count > 0 {
            x += (self.sizer.width_or_zero(0) + BORDER_WIDTH) as u16;
            let (horizontal_offset, _) = self.offset;
            let start_index = horizontal_offset + 1;
            let end_index = start_index + count - 1;
            for index in start_index..end_index {
                x += (self.sizer.width_or_zero(index) + BORDER_WIDTH) as u16;
            }
        }
        Goto(x, y)
    }

    fn write_column_rule(&self, out: &mut dyn Write, index: usize, separator: &str) -> Result<()> {
        let (hlen, hrule) = &self.hrule;
        let column_width = self.sizer.width_or_zero(index);
        let hrule_len = column_width * hlen;
        write!(out, "{}{}", separator, &hrule[0..hrule_len])
    }

    fn horizontal_rule(
        &self,
        out: &mut dyn Write,
        pos: Goto,
        start_col: usize,
        left: &'static str,
        middle: &'static str,
        right: &'static str,
    ) -> Result<()> {
        let (horizontal_offset, _) = self.offset;
        write!(out, "{}", pos)?;
        let mut start_col = start_col;
        let mut separator = left;
        if start_col == 0 {
            write!(out, "{}", pos)?;
            self.write_column_rule(out, 0, separator)?;
            start_col = 1;
            separator = middle;
        } else {
            write!(out, "{}", self.skip_columns(pos, start_col))?;
        }
        start_col += horizontal_offset;
        let column_count = start_col + self.visible_columns;
        for index in start_col..column_count {
            self.write_column_rule(out, index, separator)?;
            separator = middle;
        }
        write!(out, "{}", right)
    }

    /// Top line of the table
    pub fn top_line(&self, out: &mut dyn Write, pos: Goto) -> Result<()> {
        self.horizontal_rule(
            out,
            pos,
            1,
            self.charset.get(TableChar::DownRight),
            self.charset.get(TableChar::DownHorizontal),
            self.charset.get(TableChar::DownLeft),
        )
    }

    /// Line between the header and the body
    pub fn middle_line(&self, out: &mut dyn Write, pos: Goto) -> Result<()> {
        self.horizontal_rule(
            out,
            pos,
            0,
            self.charset.get(TableChar::DownRight),
            self.charset.get(TableChar::VerticalHorizontal),
            self.charset.get(TableChar::VerticalLeft),
        )
    }

    /// Top line of the table
    pub fn bottom_line(&self, out: &mut dyn Write, pos: Goto) -> Result<()> {
        self.horizontal_rule(
            out,
            pos,
            0,
            self.charset.get(TableChar::UpRight),
            self.charset.get(TableChar::UpHorizontal),
            self.charset.get(TableChar::UpLeft),
        )
    }

    pub fn write_horizontal_header<I, S>(
        &self,
        out: &mut dyn Write,
        pos: Goto,
        row: I,
    ) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let (horizontal_offset, _) = self.offset;
        let offset = horizontal_offset + 1;
        let pos = self.skip_columns(pos, 1);
        write!(out, "{}", pos)?;
        for (index, value) in row.into_iter().take(self.visible_columns).enumerate() {
            let width = self.sizer.width_or_zero(index + offset);
            write!(
                out,
                "{}{:^width$}",
                self.vline,
                value.as_ref(),
                width = width
            )?;
        }
        write!(out, "{}", self.vline)
    }

    fn write_column<I, S, F>(
        &self,
        out: &mut dyn Write,
        pos: Goto,
        index: usize,
        column: I,
        write_value: F,
    ) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
        F: Fn(&mut dyn Write, &str, usize) -> Result<()>,
    {
        let (horizontal_offset, vertical_offset) = self.offset;
        let (_, screen_height) = self.screen_size;
        let Goto(x, mut y) = pos;
        let right = if index + 1 >= self.visible_columns {
            self.vline
        } else {
            ""
        };
        let width = self.sizer.width_or_zero(index + horizontal_offset);
        for value in column.into_iter().skip(vertical_offset) {
            if y > screen_height {
                break;
            }
            write!(out, "{}{}", Goto(x, y), self.vline)?;
            write_value(out, value.as_ref(), width)?;
            write!(out, "{}", right)?;
            y += 1;
        }
        Ok(())
    }

    pub fn write_left_column<I, S>(&self, out: &mut dyn Write, pos: Goto, column: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        fn write_value(out: &mut dyn Write, value: &str, width: usize) -> Result<()> {
            write!(out, "{:<width$}", value, width = width)
        }
        self.write_column(out, pos, 0, column, write_value)
    }

    pub fn write_middle_column<I, S>(
        &self,
        out: &mut dyn Write,
        pos: Goto,
        index: usize,
        column: I,
    ) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        fn write_value(out: &mut dyn Write, value: &str, width: usize) -> Result<()> {
            write!(out, "{:>width$}", value, width = width)
        }
        let pos = self.skip_columns(pos, index);
        self.write_column(out, pos, index, column, write_value)
    }
}
