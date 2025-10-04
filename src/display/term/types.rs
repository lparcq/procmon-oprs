// Oprs -- process monitor for Linux
// Copyright (C) 2024-2025  Laurent Pelecq
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

use smart_default::SmartDefault;
use std::cmp;

macro_rules! void {
    ($e:expr) => {{
        let _ = $e;
    }};
}

/// Boolean properties applied to a 2-dimensions area.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Area<T> {
    pub(crate) horizontal: T,
    pub(crate) vertical: T,
}

impl<T> Area<T> {
    pub fn new(horizontal: T, vertical: T) -> Self {
        Self {
            horizontal,
            vertical,
        }
    }
}

/// Compute the maximum length of strings
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MaxLength(u16);

impl MaxLength {
    pub(crate) fn with_lines<'a, I>(items: I) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        let mut ml = Self(0);
        ml.check_lines(items);
        ml
    }

    /// The length:
    pub(crate) fn len(&self) -> u16 {
        let Self(length) = self;
        *length
    }

    /// Count the maximun length of a string
    pub(crate) fn check(&mut self, s: &str) {
        self.set_min(s.len());
    }

    pub(crate) fn max(self, other: Self) -> Self {
        if self.0 < other.0 {
            other
        } else {
            self
        }
    }

    /// Check the length of each lines.
    pub(crate) fn check_lines<'a, I>(&mut self, items: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        for item in items.into_iter() {
            self.check(item);
        }
    }

    /// Ensure a minimum length
    pub(crate) fn set_min(&mut self, l: usize) {
        let l = l as u16;
        if l > self.0 {
            self.0 = l
        }
    }
}

impl From<usize> for MaxLength {
    fn from(value: usize) -> Self {
        Self(value as u16)
    }
}

impl From<&str> for MaxLength {
    fn from(s: &str) -> Self {
        Self::from(s.len())
    }
}

/// Horizontal or vertical scrolling.
///
/// The position is a number of characters horizontally or lines vertically.
/// The page depends on the rendered component.
#[derive(Debug, Clone, Copy, SmartDefault)]
pub(crate) enum Scroll {
    /// No move
    #[default]
    CurrentPosition,
    /// First position.
    FirstPosition,
    /// Last position.
    LastPosition,
    /// Previous position from the current one.
    PreviousPosition,
    /// Next position from the current one.
    NextPosition,
    /// Previous page from the current one.
    PreviousPage,
    /// Next page from the current one.
    NextPage,
    /// Up in a hierarchy
    Up,
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Motion {
    pub(crate) position: usize,
    pub(crate) scroll: Scroll,
}

impl Motion {
    /// New motion with same scroll but different position.
    pub(crate) fn with_position(&self, position: usize) -> Self {
        let scroll = self.scroll;
        Self { position, scroll }
    }

    /// Move to a position and clear the scroll.
    pub(crate) fn move_to(&mut self, position: usize) {
        self.position = position;
        self.current();
    }

    pub(crate) fn current(&mut self) {
        self.scroll = Scroll::CurrentPosition;
    }

    pub(crate) fn first(&mut self) {
        self.scroll = Scroll::FirstPosition;
    }

    pub(crate) fn last(&mut self) {
        self.scroll = Scroll::LastPosition;
    }

    pub(crate) fn previous(&mut self) {
        self.scroll = Scroll::PreviousPosition;
    }

    pub(crate) fn next(&mut self) {
        self.scroll = Scroll::NextPosition;
    }

    pub(crate) fn previous_page(&mut self) {
        self.scroll = Scroll::PreviousPage;
    }

    pub(crate) fn next_page(&mut self) {
        self.scroll = Scroll::NextPage;
    }

    pub(crate) fn up(&mut self) {
        self.scroll = Scroll::Up;
    }

    /// Resolve the position according to the last position and total_length.
    pub(crate) fn resolve(&self, last_position: usize, page_length: usize) -> usize {
        match self.scroll {
            Scroll::CurrentPosition => self.position,
            Scroll::FirstPosition => 0,
            Scroll::LastPosition => last_position,
            Scroll::PreviousPosition => self.position.saturating_sub(1),
            Scroll::NextPosition => cmp::min(last_position, self.position + 1),
            Scroll::PreviousPage => self.position.saturating_sub(page_length),
            Scroll::NextPage => cmp::min(last_position, self.position + page_length),
            Scroll::Up => panic!("cannot resolve hierarchical moves"),
        }
    }

    /// Resolve the position according to the last position and total_length.
    pub(crate) fn update(&mut self, last_position: usize, page_length: usize) {
        self.move_to(self.resolve(last_position, page_length));
    }
}
