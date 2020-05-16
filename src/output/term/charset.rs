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

/// Characters to draw a table
const ASCII_TABLE_CHARS_: [char; 11] = ['-', '|', '+', '+', '+', '+', '+', '+', '-', '-', '+'];
const UTF8_TABLE_CHARS__: [char; 11] = ['─', '│', '┌', '┐', '└', '┘', '├', '┤', '┬', '┴', '┼'];

pub enum TableChar {
    Horizontal,
    Vertical,
    DownRight,
    DownLeft,
    UpRight,
    UpLeft,
    VerticalRight,
    VerticalLeft,
    DownHorizontal,
    UpHorizontal,
    VerticalHorizontal,
}

pub struct TableCharSet(&'static [char; 11]);

impl TableCharSet {
    pub fn new() -> TableCharSet {
        TableCharSet(if super::is_unicode() {
            &UTF8_TABLE_CHARS__
        } else {
            &ASCII_TABLE_CHARS_
        })
    }

    pub fn get(&self, kind: TableChar) -> char {
        let TableCharSet(charset) = self;
        charset[match kind {
            TableChar::Horizontal => 0usize,
            TableChar::Vertical => 1usize,
            TableChar::DownRight => 2usize,
            TableChar::DownLeft => 3usize,
            TableChar::UpRight => 4usize,
            TableChar::UpLeft => 5usize,
            TableChar::VerticalRight => 6usize,
            TableChar::VerticalLeft => 7usize,
            TableChar::DownHorizontal => 8usize,
            TableChar::UpHorizontal => 9usize,
            TableChar::VerticalHorizontal => 10usize,
        }]
    }
}
