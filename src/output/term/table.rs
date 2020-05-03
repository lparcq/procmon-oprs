use std::cmp;
use std::io::{Result, Write};
use std::iter::IntoIterator;
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
    horizontal_header: Vec<String>,
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
            vertical_header: Vec::new(),
            vertical_header_width: 0,
            columns: Vec::new(),
            horizontal_offset: 0,
            vertical_offset: 0,
        }
    }

    pub fn set_vertical_header<I>(&mut self, header: I)
    where
        I: IntoIterator<Item = String>,
    {
        self.vertical_header.clear();
        self.vertical_header.extend(header);
        self.vertical_header_width = strings_max_len(self.vertical_header.iter(), 0);
    }

    pub fn set_horizontal_header<I>(&mut self, header: I)
    where
        I: IntoIterator<Item = String>,
    {
        self.horizontal_header.clear();
        self.horizontal_header.extend(header);
        self.vertical_header_width = strings_max_len(self.vertical_header.iter(), 0);
    }

    pub fn clear_columns(&mut self) {
        self.columns.clear();
    }

    pub fn set_empty_column(&mut self, col_num: usize) {
        let empty = Vec::<String>::new();
        self.set_column(col_num, empty.iter().map(|s| s.to_string()));
    }

    pub fn set_column<I>(&mut self, col_num: usize, values: I)
    where
        I: IntoIterator<Item = String>,
    {
        let column_count = self.columns.len();
        match col_num.cmp(&column_count) {
            cmp::Ordering::Equal => {
                let mut column = Vec::new();
                column.extend(values);
                self.columns.push(column);
            }
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
                strings_max_len(col.iter(), self.horizontal_header[col_num].len())
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

        for (width, title) in column_widths
            .iter()
            .zip(self.horizontal_header.iter())
            .skip(self.horizontal_offset)
        {
            if *width > 0 {
                write!(out, " {:^width$}", title, width = *width)?;
            }
        }
        let empty_string = String::from("");
        for (row_num, title) in self
            .vertical_header
            .iter()
            .enumerate()
            .skip(self.vertical_offset)
            .take(cmp::min((height as usize) - 1, self.vertical_header.len()))
        {
            y += 1;
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
        }
        Ok(())
    }
}
