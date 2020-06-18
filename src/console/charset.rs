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

/// Characters to draw a table in ASCII
const ASCII_TABLE_CHARS_: [&str; 15] = [
    "-", "|", "+", "+", "+", "+", "+", "+", "-", "-", "+", "<", "^", ">", "v",
];

/// Characters to draw a table in UTF8
const UTF8_TABLE_CHARS__: [&str; 15] = [
    "─", "│", "┌", "┐", "└", "┘", "├", "┤", "┬", "┴", "┼", "←", "↑", "→", "↓",
];

/// Character types to draw a table
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
    ArrowLeft,
    ArrowUp,
    ArrowRight,
    ArrowDown,
}

/// Check if charset is unicode
pub fn is_unicode() -> bool {
    if let Ok(lang) = std::env::var("LANG") {
        lang.to_lowercase().contains(".utf")
    } else {
        false
    }
}

/// Characters to draw a table.
pub struct TableCharSet(&'static [&'static str; 15]);

impl TableCharSet {
    pub fn new() -> TableCharSet {
        TableCharSet(if is_unicode() {
            &UTF8_TABLE_CHARS__
        } else {
            &ASCII_TABLE_CHARS_
        })
    }

    /// Get a specific character to draw a table
    pub fn get(&self, kind: TableChar) -> &'static str {
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
            TableChar::ArrowLeft => 11usize,
            TableChar::ArrowUp => 12usize,
            TableChar::ArrowRight => 13usize,
            TableChar::ArrowDown => 14usize,
        }]
    }

    /// Strings made of the same character multiple times.
    pub fn repeat(&self, kind: TableChar, count: usize) -> (usize, String) {
        let unit = self.get(kind);
        (unit.len(), unit.repeat(count))
    }

    /// An horizontal line of a given size.
    pub fn horizontal_line(&self, count: usize) -> (usize, String) {
        self.repeat(TableChar::Horizontal, count)
    }
}
