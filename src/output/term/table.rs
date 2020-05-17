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
    ScreenSize,
};

/// Crosstab widget
///
/// Table with horizontal header and vertical header
pub struct TableDrawer<'a, 'b> {
    charset: &'a TableCharSet,
    sizer: &'b ColumnSizer,
    screen_size: ScreenSize,
    offset: (usize, usize),
    hrule: (usize, String),
    vline: &'static str,
}

impl<'a, 'b> TableDrawer<'a, 'b> {
    pub fn new(
        charset: &'a TableCharSet,
        sizer: &'b ColumnSizer,
        screen_size: ScreenSize,
        offset: (usize, usize),
    ) -> TableDrawer<'a, 'b> {
        let vline: &'static str = charset.get(TableChar::Vertical);
        let max_col_width = sizer.iter().max().unwrap_or(&0);
        dbg!(charset.get(TableChar::Horizontal).len());
        let hrule = charset.horizontal_line(*max_col_width);
        TableDrawer {
            sizer,
            screen_size,
            offset,
            charset,
            hrule,
            vline,
        }
    }

    fn skip_columns(&self, pos: Goto, count: usize) -> Goto {
        let Goto(mut x, y) = pos;
        for index in 0..count {
            x += (self.sizer.width_or_zero(index) + 1) as u16;
        }
        Goto(x, y)
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
        let pos = self.skip_columns(pos, start_col);
        write!(out, "{}", pos)?;
        let (hlen, hrule) = &self.hrule;
        for index in start_col..self.sizer.len() {
            let separator = if index == start_col { left } else { middle };
            let column_width = self.sizer.width_or_zero(index);
            let hrule_len = column_width * hlen;
            write!(out, "{}{}", separator, &hrule[0..hrule_len])?;
        }
        write!(out, "{}", right)?;
        Ok(())
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

    pub fn write_horizontal_header1<I, S>(
        &self,
        out: &mut dyn Write,
        pos: Goto,
        row: I,
    ) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let offset = 1;
        let pos = self.skip_columns(pos, offset);
        write!(out, "{}", pos)?;
        for (index, value) in row.into_iter().enumerate() {
            let width = self.sizer.width_or_zero(index + offset);
            write!(
                out,
                "{}{:^width$}",
                self.vline,
                value.as_ref(),
                width = width
            )?;
        }
        write!(out, "{}", self.vline)?;
        Ok(())
    }

    fn write_column<I, S, F>(
        &self,
        out: &mut dyn Write,
        pos: Goto,
        index: usize,
        column: I,
        write_value: F,
    ) -> Result<bool>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
        F: Fn(&mut dyn Write, &str, usize) -> Result<()>,
    {
        let pos = self.skip_columns(pos, index);
        let (_, screen_height) = self.screen_size;
        let (_, vertical_offset) = self.offset;
        let Goto(x, mut y) = pos;
        let right = if index + 1 >= self.sizer.len() {
            self.vline
        } else {
            ""
        };
        let width = self.sizer.width_or_zero(index);
        for value in column.into_iter().skip(vertical_offset) {
            if y > screen_height {
                return Ok(true);
            }
            write!(out, "{}{}", Goto(x, y), self.vline)?;
            write_value(out, value.as_ref(), width)?;
            write!(out, "{}", right)?;
            y += 1;
        }
        Ok(false)
    }

    pub fn write_left_column<I, S>(&self, out: &mut dyn Write, pos: Goto, column: I) -> Result<bool>
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
    ) -> Result<bool>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        fn write_value(out: &mut dyn Write, value: &str, width: usize) -> Result<()> {
            write!(out, "{:>width$}", value, width = width)
        }
        self.write_column(out, pos, index, column, write_value)
    }
}
