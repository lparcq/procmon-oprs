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

use std::cmp;
use std::io::{Result, Write};
use std::iter::{IntoIterator, Iterator};
use std::slice;
use termion::{clear, cursor::Goto};

use super::widget::{Size, Widget};

fn strings_max_len(iter: slice::Iter<String>, initial: usize) -> usize {
    iter.fold(initial, |max, s| cmp::max(max, s.len()))
}

/// Crosstab widget
///
/// Table with horizontal header and vertical header
pub struct TableWidget {
    horizontal_header: Vec<Vec<String>>,
    horizontal_header_widths: Vec<usize>,
    vertical_header: Vec<String>,
    vertical_header_width: usize,
    columns: Vec<Vec<String>>,
    horizontal_offset: usize,
    vertical_offset: usize,
}

impl TableWidget {
    pub fn new() -> TableWidget {
        TableWidget {
            horizontal_header: Vec::new(),
            horizontal_header_widths: Vec::new(),
            vertical_header: Vec::new(),
            vertical_header_width: 0,
            columns: Vec::new(),
            horizontal_offset: 0,
            vertical_offset: 0,
        }
    }

    pub fn set_vertical_header<I>(&mut self, header: I)
    where
        I: IntoIterator<Item = &'static str>,
    {
        self.vertical_header.clear();
        self.vertical_header
            .extend(header.into_iter().map(|s| s.to_string()));
        self.vertical_header_width = strings_max_len(self.vertical_header.iter(), 0);
    }

    pub fn clear_horizontal_header(&mut self) {
        self.horizontal_header.clear();
        self.horizontal_header_widths.clear();
    }

    pub fn append_horizontal_header<I>(&mut self, header: I)
    where
        I: Iterator<Item = String>,
    {
        let row = header
            .enumerate()
            .map(|(col_num, hdr)| {
                let hdr_len = hdr.len();
                if col_num >= self.horizontal_header_widths.len() {
                    self.horizontal_header_widths.push(hdr_len);
                } else {
                    let width = self.horizontal_header_widths[col_num];
                    if hdr_len > width {
                        self.horizontal_header_widths[col_num] = hdr_len;
                    }
                }
                hdr
            })
            .collect();
        self.horizontal_header.push(row);
    }

    pub fn clear_columns(&mut self) {
        self.columns.clear();
    }

    pub fn set_column<I>(&mut self, col_num: usize, values: I)
    where
        I: IntoIterator<Item = String>,
    {
        let column_count = self.columns.len();
        match col_num.cmp(&column_count) {
            cmp::Ordering::Equal => self.columns.push(values.into_iter().collect()),
            cmp::Ordering::Less => self.columns[col_num] = values.into_iter().collect(),
            _ => panic!("internal error"),
        }
    }

    /// Calculate the column width, remove columns beyond the max width and truncate the last column if needed.
    fn column_widths(&self, max_width: usize) -> Vec<usize> {
        let mut column_widths: Vec<usize> = self
            .columns
            .iter()
            .enumerate()
            .skip(self.horizontal_offset)
            .map(|(col_num, col)| {
                strings_max_len(col.iter(), self.horizontal_header_widths[col_num])
            })
            .collect();
        // total_width is the width of columns plus one char in between
        let mut total_width = column_widths.iter().sum::<usize>() + column_widths.len() - 1;
        while total_width > max_width {
            if column_widths.is_empty() {
                total_width = 0;
            } else {
                let last_width = *(column_widths.last().unwrap());
                if total_width - last_width > max_width {
                    column_widths.pop().unwrap();
                    if column_widths.is_empty() {
                        total_width = 0;
                    } else {
                        total_width -= last_width + 1;
                    }
                } else {
                    *(column_widths.last_mut().unwrap()) -= total_width - max_width;
                }
            }
        }
        column_widths
    }
}

impl Widget for TableWidget {
    fn write(&self, out: &mut dyn Write, pos: Goto, size: Size) -> Result<()> {
        let Goto(x, mut y) = pos;
        let body_pos_x = x + self.vertical_header_width as u16;
        let (width, height) = size;
        let column_widths = self.column_widths(width as usize);

        write!(out, "{}{}{}", pos, clear::CurrentLine, Goto(body_pos_x, y))?;

        for header in &self.horizontal_header {
            for (width, title) in column_widths
                .iter()
                .zip(header.iter())
                .skip(self.horizontal_offset)
            {
                if *width > 0 {
                    write!(out, " {:^width$}", title, width = *width)?;
                }
            }
            y += 1;
            write!(out, "{}", Goto(body_pos_x, y))?;
        }
        let empty_string = String::from("");
        for (row_num, title) in self
            .vertical_header
            .iter()
            .enumerate()
            .skip(self.vertical_offset)
            .take(cmp::min((height as usize) - 1, self.vertical_header.len()))
        {
            write!(
                out,
                "{}{:<width$}",
                Goto(x, y),
                title,
                width = self.vertical_header_width
            )?;
            for (col_num, width) in column_widths.iter().enumerate() {
                if *width > 0 {
                    write!(
                        out,
                        " {:>width$}",
                        self.columns
                            .get(col_num)
                            .map_or("", |column| column.get(row_num).unwrap_or(&empty_string)),
                        width = width
                    )?;
                }
            }
            y += 1;
        }
        Ok(())
    }
}
