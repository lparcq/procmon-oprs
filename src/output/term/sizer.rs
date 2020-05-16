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

use std::cmp::{max, min};
use std::iter::{IntoIterator, Iterator};
use std::slice::Iter;

/// Compute column widths of a table.
///
/// Widths of columns may be directly written or calculated from the column
/// content. The leftmost column to redraw is returned when calling freeze.
pub struct ColumnSizer {
    elasticity: usize,
    widths: Vec<usize>,
    changed_from: Option<usize>,
}

impl ColumnSizer {
    pub fn new(elasticity: usize) -> ColumnSizer {
        ColumnSizer {
            elasticity,
            widths: Vec::new(),
            changed_from: None,
        }
    }

    /// Return the index of the leftmost column that has changed.
    /// If the leftmost change is a column removal, the index is the number of columns.
    pub fn freeze(&mut self) -> Option<usize> {
        let changed_from = self.changed_from;
        self.changed_from = None;
        changed_from
    }

    /// Number of columns.
    pub fn len(&self) -> usize {
        self.widths.len()
    }

    /// Column width.
    pub fn width(&self, index: usize) -> Option<usize> {
        self.widths.get(index).copied()
    }

    /// Column width or zero if the index is out of range.
    pub fn width_or_zero(&self, index: usize) -> usize {
        self.width(index).unwrap_or(0)
    }

    /// Iterate over the columns width.
    pub fn iter(&self) -> Iter<usize> {
        self.widths.iter()
    }

    /// Replace the width of a given column or insert it.
    pub fn overwrite(&mut self, index: usize, width: usize) {
        while index > self.widths.len() {
            self.widths.push(0);
        }
        let mut changed = false;
        if let Some(width_ref) = self.widths.get_mut(index) {
            let current_width = *width_ref;
            if width > current_width
                || (current_width >= self.elasticity && width < current_width - self.elasticity)
            {
                *width_ref = width;
                changed = true;
            }
        } else {
            self.widths.push(width);
            changed = true;
        }
        if changed && (self.changed_from.is_none() || index < self.changed_from.unwrap()) {
            self.changed_from = Some(index);
        }
    }

    /// Set the minimun width of a given column or insert it.
    pub fn overwrite_min(&mut self, index: usize, width: usize) {
        self.overwrite(index, min(width, self.width_or_zero(index)));
    }

    /// Append the width of a given column.
    pub fn push(&mut self, width: usize) {
        self.overwrite(self.widths.len(), width);
    }

    /// Remove the width of the last column.
    pub fn pop(&mut self) {
        if let Some(_) = self.widths.pop() {
            self.changed_from = Some(self.widths.len());
        }
    }

    /// Replace the width of a given column or insert it by calculating the largest string length.
    pub fn overwrite_column<'a, I>(&mut self, index: usize, column: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        let width = column.into_iter().fold(0, |acc, s| max(acc, s.len()));
        self.overwrite(index, width);
    }

    /// Append the width of a given column or insert it by calculating the largest string length.
    pub fn push_column<'a, I>(&mut self, column: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        self.overwrite_column(self.widths.len(), column);
    }
}

#[cfg(test)]
mod tests {

    use super::ColumnSizer;

    const COL1: [&'static str; 3] = ["Name", "Ada Lovelace", "Charles Babbage"];
    static WIDTH1: usize = COL1[2].len();
    const COL2: [&'static str; 3] = ["Birth date", "10 December 1815", "26 December 1791"];
    static WIDTH2: usize = COL2[1].len();
    const COL3: [&'static str; 3] = ["Known for", "Mathematics, computing", "Difference engine"];
    static WIDTH3: usize = COL3[1].len();

    #[test]
    fn test_column_sizer_push() {
        let mut csizer = ColumnSizer::new(0);
        assert_eq!(0, csizer.len());
        // add the first column
        csizer.push_column(COL1.iter().copied());
        assert_eq!(1, csizer.len());
        assert_eq!(WIDTH1, csizer.width(0).unwrap());
        // add the third column
        csizer.overwrite_column(2, COL3.iter().copied());
        assert_eq!(3, csizer.len());
        assert_eq!(0, csizer.width(1).unwrap());
        assert_eq!(WIDTH3, csizer.width_or_zero(2));
        assert_eq!(Some(0), csizer.freeze());
        // add the second column
        csizer.overwrite_column(1, COL2.iter().copied());
        assert_eq!(3, csizer.len());
        assert_eq!(WIDTH2, csizer.width_or_zero(1));
        assert_eq!(Some(1), csizer.freeze());
        // list all width
        assert_eq!(
            &[WIDTH1, WIDTH2, WIDTH3],
            csizer.iter().copied().collect::<Vec<usize>>().as_slice()
        );
    }

    #[test]
    fn test_column_sizer_pop() {
        let mut csizer = ColumnSizer::new(0);
        csizer.push_column(COL1.iter().copied());
        csizer.push_column(COL2.iter().copied());
        csizer.push_column(COL3.iter().copied());
        assert_eq!(Some(0), csizer.freeze());
        csizer.pop();
        assert_eq!(Some(csizer.len()), csizer.freeze());
    }

    #[test]
    fn test_column_sizer_growing() {
        const ELASTICITY: usize = 1;
        let mut csizer = ColumnSizer::new(ELASTICITY);
        csizer.push(10);
        csizer.push(7);
        csizer.push(9);
        assert_eq!(7, csizer.width_or_zero(1));
        assert_eq!(Some(0), csizer.freeze());
        csizer.overwrite(1, 12);
        assert_eq!(12, csizer.width_or_zero(1));
        assert_eq!(Some(1), csizer.freeze());
    }

    #[test]
    fn test_column_sizer_shrinking() {
        const ELASTICITY: usize = 2;
        let mut csizer = ColumnSizer::new(ELASTICITY);
        csizer.push(10);
        csizer.push(12);
        csizer.push(9);
        assert_eq!(12, csizer.width_or_zero(1));
        // No change if column shrink a little bit
        assert_eq!(Some(0), csizer.freeze());
        csizer.overwrite(1, 10);
        assert_eq!(12, csizer.width_or_zero(1));
        // Change if column shrink significantly
        csizer.overwrite(1, 9);
        assert_eq!(9, csizer.width_or_zero(1));
    }
}
