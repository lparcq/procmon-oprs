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

/// Characters used to draw an horizontal line
struct HorizontalLineChars {
    left: &'static str,
    middle: &'static str,
    right: &'static str,
}

impl HorizontalLineChars {
    fn top_border(chars: &TableCharSet) -> HorizontalLineChars {
        HorizontalLineChars {
            left: chars.get(TableChar::DownRight),
            middle: chars.get(TableChar::DownHorizontal),
            right: chars.get(TableChar::DownLeft),
        }
    }

    fn middle_border(chars: &TableCharSet) -> HorizontalLineChars {
        HorizontalLineChars {
            left: chars.get(TableChar::DownRight),
            middle: chars.get(TableChar::VerticalHorizontal),
            right: chars.get(TableChar::VerticalLeft),
        }
    }

    fn bottom_border(chars: &TableCharSet) -> HorizontalLineChars {
        HorizontalLineChars {
            left: chars.get(TableChar::UpRight),
            middle: chars.get(TableChar::UpHorizontal),
            right: chars.get(TableChar::UpLeft),
        }
    }
}

struct Graphics {
    border_vertical_line: &'static str,
    vertical_line: &'static str,
    border_hozizontal_rule: (usize, String),
    horizontal_rule: (usize, String),
    top_border: HorizontalLineChars,
    middle_border: HorizontalLineChars,
    bottom_border: HorizontalLineChars,
}

impl Graphics {
    fn new(chars: &TableCharSet, max_col_width: usize) -> Graphics {
        Graphics {
            border_vertical_line: chars.get(TableChar::Vertical),
            vertical_line: chars.get(TableChar::VerticalInner),
            border_hozizontal_rule: chars.outter_horizontal_line(max_col_width),
            horizontal_rule: chars.inner_horizontal_line(max_col_width),
            top_border: HorizontalLineChars::top_border(chars),
            middle_border: HorizontalLineChars::middle_border(chars),
            bottom_border: HorizontalLineChars::bottom_border(chars),
        }
    }
}

/// Crosstab widget
///
/// Table with horizontal header and vertical header
pub struct TableDrawer<'b> {
    sizer: &'b ColumnSizer,
    screen_size: Size,
    offset: (usize, usize),
    visible_columns: usize,
    border_width: usize,
    graphics: Graphics,
}

impl<'b> TableDrawer<'b> {
    pub fn new(
        chars: &TableCharSet,
        sizer: &'b ColumnSizer,
        screen_size: Size,
        offset: (usize, usize),
        visible_columns: usize,
    ) -> TableDrawer<'b> {
        let max_col_width = sizer.iter().max().unwrap_or(&0);
        let border_width = chars.border_width;
        let graphics = Graphics::new(chars, *max_col_width);
        TableDrawer {
            sizer,
            screen_size,
            offset,
            visible_columns,
            border_width,
            graphics,
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

    fn write_column_rule(
        &self,
        screen: &mut Screen,
        index: usize,
        separator: &str,
        hlen: usize,
        hrule: &str,
    ) -> Result<()> {
        let column_width = self.sizer.width_or_zero(index);
        let hrule_len = column_width * hlen;
        write!(screen, "{}{}", separator, &hrule[0..hrule_len])
    }

    fn horizontal_rule(
        &self,
        screen: &mut Screen,
        origin: Origin,
        start_col: usize,
        border_chars: &HorizontalLineChars,
        hrule: &(usize, String),
    ) -> Result<()> {
        let (horizontal_offset, _) = self.offset;
        let mut start_col = start_col;
        let mut separator = border_chars.left;
        let (unit_len, hrule) = hrule;
        if start_col == 0 {
            screen.origin(origin)?;
            self.write_column_rule(screen, 0, separator, *unit_len, hrule)?;
            start_col = 1;
            separator = border_chars.middle;
        } else {
            let Origin(x, y) = origin;
            screen.goto(self.skip_columns(x, start_col), y)?;
        }
        start_col += horizontal_offset;
        let column_count = start_col + self.visible_columns;
        for index in start_col..column_count {
            self.write_column_rule(screen, index, separator, *unit_len, hrule)?;
            separator = border_chars.middle;
        }
        write!(screen, "{}", border_chars.right)
    }

    /// Top line of the table
    pub fn top_line(&self, screen: &mut Screen, pos: Origin) -> Result<()> {
        self.horizontal_rule(
            screen,
            pos,
            1,
            &self.graphics.top_border,
            &self.graphics.border_hozizontal_rule,
        )
    }

    /// Line between the header and the body
    pub fn middle_line(&self, screen: &mut Screen, pos: Origin) -> Result<()> {
        self.horizontal_rule(
            screen,
            pos,
            0,
            &self.graphics.middle_border,
            &self.graphics.horizontal_rule,
        )
    }

    /// Top line of the table
    pub fn bottom_line(&self, screen: &mut Screen, pos: Origin) -> Result<()> {
        self.horizontal_rule(
            screen,
            pos,
            0,
            &self.graphics.bottom_border,
            &self.graphics.border_hozizontal_rule,
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
            write!(screen, "{}", self.graphics.vertical_line)?;
            if bold {
                screen.bold()?;
            }
            write!(screen, "{:^width$}", value.as_ref(), width = width)?;
            if bold {
                screen.style_reset()?;
            }
        }
        write!(screen, "{}", self.graphics.vertical_line)
    }

    fn write_column<F>(
        &self,
        screen: &mut Screen,
        pos: Origin,
        index: usize,
        column: &[Cell<'_>],
        left_line: &'static str,
        right_line: &'static str,
        write_value: F,
    ) -> Result<()>
    where
        F: Fn(&mut Screen, &str, usize) -> Result<()>,
    {
        let (horizontal_offset, vertical_offset) = self.offset;
        let Size(_, screen_height) = self.screen_size;
        let Origin(x, mut y) = pos;
        let next_index = index + 1;
        let right = if next_index >= self.visible_columns {
            right_line
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
            write!(screen, "{}", left_line)?;
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
        self.write_column(
            screen,
            pos,
            0,
            column,
            self.graphics.border_vertical_line,
            self.graphics.vertical_line,
            write_value,
        )
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
        let right_line = if index + 1 == self.sizer.len() {
            self.graphics.border_vertical_line
        } else {
            self.graphics.vertical_line
        };
        self.write_column(
            screen,
            pos,
            index,
            column,
            self.graphics.vertical_line,
            right_line,
            write_value,
        )
    }
}
