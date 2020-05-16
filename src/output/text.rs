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

use std::thread;
use std::time::Duration;

use super::term::{TableChar, TableCharSet};
use super::Output;
use crate::agg::Aggregation;
use crate::collector::Collector;

const REPEAT_HEADER_EVERY: u16 = 20;
const RESIZE_IF_COLUMNS_SHRINK: usize = 2;

fn divide(numerator: usize, denominator: usize) -> (usize, usize) {
    let quotient = numerator / denominator;
    (quotient, numerator - quotient * denominator)
}

/// A subtitle with a name and a short name
struct SubTitle {
    name: &'static str,
    short_name: Option<&'static str>,
}

/// Space between the vertical lines and the text
const VERTICAL_PADDING: usize = 1;

/// Print an infinite table
struct Table {
    titles: Vec<String>,
    subtitles: Vec<SubTitle>,
    values: Vec<String>,
    title_count: usize,
    title_width: usize,
    column_width: usize,
    repeat: u16,
    charset: TableCharSet,
    hrule: String,
    vertical_padding: String,
}

impl Table {
    fn new() -> Table {
        let charset = TableCharSet::new();
        Table {
            titles: Vec::new(),
            subtitles: Vec::new(),
            values: Vec::new(),
            title_count: 0,
            title_width: 0,
            column_width: 0,
            repeat: 0,
            charset,
            hrule: String::from(""),
            vertical_padding: " ".repeat(VERTICAL_PADDING),
        }
    }

    fn clear_titles(&mut self) {
        self.titles.clear();
    }

    fn push_title(&mut self, title: String) {
        self.titles.push(title);
    }

    fn push_subtitle(&mut self, name: &'static str, short_name: Option<&'static str>) {
        self.subtitles.push(SubTitle { name, short_name });
    }

    fn clear_values(&mut self) {
        self.values.clear();
    }

    fn push_value(&mut self, value: &str) {
        self.values.push(value.to_string());
    }

    fn horizontal_rule(&self, left: char, middle_title: char, middle_subtitle: char, right: char) {
        let subtitle_count = self.subtitles.len();
        let column_count = subtitle_count * self.title_count;
        for index in 0..column_count {
            let separator = if index == 0 {
                left
            } else if index % subtitle_count == 0 {
                middle_title
            } else {
                middle_subtitle
            };
            print!("{}{}", separator, self.hrule);
        }
        println!("{}", right);
    }

    fn print_header(&mut self) {
        if self.title_count > 0 {
            self.print_footer();
        }
        self.title_count = self.titles.len();
        // An horizontal rule
        self.horizontal_rule(
            self.charset.get(TableChar::DownRight),
            self.charset.get(TableChar::DownHorizontal),
            self.charset.get(TableChar::Horizontal),
            self.charset.get(TableChar::DownLeft),
        );
        // Titles
        for title in &self.titles {
            print!(
                "|{}{:^width$}{}",
                self.vertical_padding,
                title,
                self.vertical_padding,
                width = self.title_width
            );
        }
        let vline = self.charset.get(TableChar::Vertical);
        println!("{}", vline);
        self.horizontal_rule(
            self.charset.get(TableChar::VerticalRight),
            self.charset.get(TableChar::VerticalHorizontal),
            self.charset.get(TableChar::DownHorizontal),
            self.charset.get(TableChar::VerticalLeft),
        );
        // Subtitles
        for _ in 0..self.title_count {
            for subtitle in &self.subtitles {
                print!(
                    "{}{}{:^width$}{}",
                    vline,
                    self.vertical_padding,
                    if subtitle.name.len() > self.column_width {
                        subtitle
                            .short_name
                            .expect("cannot have sub-title larger than column width")
                    } else {
                        subtitle.name
                    },
                    self.vertical_padding,
                    width = self.column_width
                );
            }
        }
        println!("{}", vline);
    }

    fn print_footer(&self) {
        self.horizontal_rule(
            self.charset.get(TableChar::UpRight),
            self.charset.get(TableChar::UpHorizontal),
            self.charset.get(TableChar::Horizontal),
            self.charset.get(TableChar::UpLeft),
        );
    }

    fn print_values(&self) {
        for value in &self.values {
            print!("| {:^width$} ", value, width = self.column_width);
        }
        println!("|");
    }

    /// Calculate the column width
    fn resize(&mut self) {
        let subtitle_count = self.subtitles.len();
        let mut column_width = 0;
        for title in &self.titles {
            // minimum column with to display the title
            let (quotient, remainder) = divide(title.len() + 3, subtitle_count);
            let min_col_width = quotient - 3 + if remainder > 0 { 1 } else { 0 };
            if min_col_width > column_width {
                column_width = min_col_width;
            }
        }
        for subtitle in &self.subtitles {
            let slen = match subtitle.short_name {
                Some(name) => name.len(),
                None => subtitle.name.len(),
            };
            if slen > column_width {
                column_width = slen;
            }
        }
        for value in &self.values {
            if value.len() > column_width {
                column_width = value.len();
            }
        }
        let title_width = (column_width + 3) * subtitle_count - 3;
        if column_width > self.column_width
            || self.column_width - column_width > RESIZE_IF_COLUMNS_SHRINK
        {
            self.column_width = column_width;
            self.title_width = title_width;
            self.repeat = 0;
        }
        self.hrule = format!("{}", self.charset.get(TableChar::Horizontal))
            .repeat(column_width + 2 * VERTICAL_PADDING);
    }

    fn print(&mut self, with_header: bool) {
        if with_header || self.repeat == 0 {
            self.print_header();
        }
        self.print_values();
        self.repeat += 1;
        if self.repeat >= REPEAT_HEADER_EVERY {
            self.repeat = 0;
        }
    }
}

/// Print on standard output as a table
pub struct TextOutput {
    every: Duration,
    table: Table,
}

impl TextOutput {
    pub fn new(every: Duration) -> TextOutput {
        TextOutput {
            every,
            table: Table::new(),
        }
    }
}

impl Output for TextOutput {
    fn open(&mut self, collector: &Collector) -> anyhow::Result<()> {
        let mut last_id = None;
        collector.for_each_computed_metric(|id, ag| {
            if last_id.is_none() || last_id.unwrap() != id {
                last_id = Some(id);
                self.table.push_subtitle(id.to_str(), id.to_short_str());
            } else {
                let subtitle = match ag {
                    Aggregation::None => "none", // never used
                    Aggregation::Min => "min",
                    Aggregation::Max => "max",
                    Aggregation::Ratio => "ratio",
                };
                self.table.push_subtitle(subtitle, None);
            }
        });
        Ok(())
    }

    fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn render(&mut self, collector: &Collector, targets_updated: bool) -> anyhow::Result<()> {
        if collector.is_empty() {
            eprintln!("no process found")
        } else {
            self.table.clear_titles();
            self.table.clear_values();
            collector.lines().for_each(|proc| {
                let name = format!("{} [{}]", proc.get_name(), proc.get_pid(),);
                self.table.push_title(name);
                proc.samples().for_each(|sample| {
                    sample
                        .strings()
                        .for_each(|value| self.table.push_value(value))
                });
            });
            self.table.resize();
            self.table.print(targets_updated);
        }
        Ok(())
    }

    fn pause(&mut self) -> anyhow::Result<bool> {
        thread::sleep(self.every);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_divide() {
        assert_eq!((2, 0), super::divide(8, 4));
        assert_eq!((3, 2), super::divide(11, 3));
    }
}
