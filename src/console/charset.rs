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

/// Check if charset is unicode
pub fn is_unicode() -> bool {
    if let Ok(lang) = std::env::var("LANG") {
        lang.to_lowercase().contains(".utf")
    } else {
        false
    }
}

/// Arrows in ASCII
const ASCII_ARROWS_: [&str; 4] = ["<", "^", ">", "v"];

/// Arrows in UTF8
const UTF8_ARROWS_: [&str; 4] = ["←", "↑", "→", "↓"];

/// Character types to draw arrows
pub enum ArrowChar {
    Left,
    Up,
    Right,
    Down,
}

/// Characters to draw arrows
pub struct ArrowCharSet(&'static [&'static str; 4]);

impl ArrowCharSet {
    pub fn new() -> ArrowCharSet {
        ArrowCharSet(if is_unicode() {
            &UTF8_ARROWS_
        } else {
            &ASCII_ARROWS_
        })
    }

    /// Get a specific character to draw a table
    pub fn get(&self, kind: ArrowChar) -> &'static str {
        let ArrowCharSet(charset) = self;
        charset[match kind {
            ArrowChar::Left => 0usize,
            ArrowChar::Up => 1usize,
            ArrowChar::Right => 2usize,
            ArrowChar::Down => 3usize,
        }]
    }
}

/// Characters to draw a table in ASCII
const ASCII_TABLE_CHARS_: [&str; 13] = [
    "-", "|", "+", "+", "+", "+", "+", "+", "-", "-", "-", "|", "+",
];

/// Characters to draw a table in UTF8
const UTF8_TABLE_CHARS_: [&str; 13] = [
    "─", "│", "┌", "┐", "└", "┘", "├", "┤", "┬", "┴", "─", "│", "┼",
];

/// Characters to draw a table without lines
const BLANK_TABLE_CHARS_: [&str; 13] = ["", "", "", "", "", "", "", "", "", "", " ", " ", " "];

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
    HorizontalInner,
    VerticalInner,
    VerticalHorizontal,
}

/// Characters to draw a table.
pub struct TableCharSet {
    chars: &'static [&'static str; 13],
    pub border_width: usize,
}

impl TableCharSet {
    pub fn new() -> TableCharSet {
        TableCharSet {
            chars: if is_unicode() {
                &UTF8_TABLE_CHARS_
            } else {
                &ASCII_TABLE_CHARS_
            },
            border_width: 1,
        }
    }

    pub fn without_lines() -> TableCharSet {
        TableCharSet {
            chars: &BLANK_TABLE_CHARS_,
            border_width: 0,
        }
    }

    /// Get a specific character to draw a table
    pub fn get(&self, kind: TableChar) -> &'static str {
        self.chars[match kind {
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
            TableChar::HorizontalInner => 10usize,
            TableChar::VerticalInner => 11usize,
            TableChar::VerticalHorizontal => 12usize,
        }]
    }

    /// Strings made of the same character multiple times.
    pub fn repeat(&self, kind: TableChar, count: usize) -> (usize, String) {
        let unit = self.get(kind);
        (unit.len(), unit.repeat(count))
    }

    /// An inner horizontal line of a given size.
    pub fn inner_horizontal_line(&self, count: usize) -> (usize, String) {
        self.repeat(TableChar::HorizontalInner, count)
    }

    /// A border horizontal line of a given size.
    pub fn outter_horizontal_line(&self, count: usize) -> (usize, String) {
        self.repeat(TableChar::Horizontal, count)
    }
}
