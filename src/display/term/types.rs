// Oprs -- process monitor for Linux
// Copyright (C) 2024  Laurent Pelecq
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

use num_traits::{ConstZero, Saturating, Zero};
use std::{
    cmp::Ordering,
    collections::VecDeque,
    ops::{Add, Sub},
};

macro_rules! void {
    ($e:expr) => {{
        let _ = $e;
    }};
}

/// Unsigned integer type with an infinite value.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Unbounded<T: Clone + Copy + Default> {
    Value(T),
    Infinite,
}

impl<T: Clone + Copy + Default> Unbounded<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Infinite => None,
        }
    }
}

impl<
        T: Clone + Copy + Default + Add<Output = T> + Sub<Output = T> + Zero + ConstZero + PartialEq,
    > Zero for Unbounded<T>
{
    fn is_zero(&self) -> bool {
        match self {
            Unbounded::Value(value) => *value == T::ZERO,
            _ => false,
        }
    }

    fn zero() -> Self {
        Self::Value(T::ZERO)
    }
}

impl<
        T: Clone + Copy + Default + Add<Output = T> + Sub<Output = T> + Zero + ConstZero + PartialEq,
    > ConstZero for Unbounded<T>
{
    const ZERO: Self = Unbounded::Value(T::ZERO);
}

impl<T: Clone + Copy + Default + Add<Output = T>> Add for Unbounded<T>
where
    T: Add,
{
    type Output = Unbounded<T::Output>;
    fn add(self, rhs: Unbounded<T>) -> Unbounded<T::Output> {
        match (self, rhs) {
            (Unbounded::Value(lhs), Unbounded::Value(rhs)) => Unbounded::Value(lhs + rhs),
            _ => Unbounded::Infinite,
        }
    }
}

impl<T: Clone + Copy + Default + Sub<Output = T> + Saturating> Sub for Unbounded<T>
where
    T: Sub,
{
    type Output = Unbounded<T::Output>;
    fn sub(self, rhs: Unbounded<T>) -> Unbounded<T::Output> {
        match (self, rhs) {
            (Unbounded::Value(lhs), Unbounded::Value(rhs)) => {
                Unbounded::Value(lhs.saturating_sub(rhs))
            }
            _ => Unbounded::Infinite,
        }
    }
}

impl<T: Clone + Copy + Default + Add<Output = T> + Sub<Output = T> + Saturating> Unbounded<T> {
    pub(crate) fn add(self, delta: T) -> Self {
        self + Unbounded::Value(delta)
    }

    pub(crate) fn sub(self, delta: T) -> Self {
        self - Unbounded::Value(delta)
    }
}

impl<T: Clone + Copy + Default + Add<Output = T> + Sub<Output = T>> Default for Unbounded<T> {
    fn default() -> Self {
        Unbounded::Value(T::default())
    }
}

impl<
        T: Clone + Copy + Default + Add<Output = T> + Sub<Output = T> + PartialOrd + Ord + Saturating,
    > PartialOrd for Unbounded<T>
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Unbounded::Value(value), Unbounded::Value(other_value)) => {
                Some(value.cmp(other_value))
            }
            (Unbounded::Value(_), Unbounded::Infinite) => Some(Ordering::Less),
            (Unbounded::Infinite, Unbounded::Value(_)) => Some(Ordering::Greater),
            (Unbounded::Infinite, Unbounded::Infinite) => None,
        }
    }
}

impl<T: Clone + Copy + Default + Add<Output = T> + Sub<Output = T> + PartialEq + Saturating>
    PartialEq for Unbounded<T>
{
    fn eq(&self, other: &Self) -> bool {
        match self {
            Unbounded::Value(value) => match other {
                Unbounded::Value(other_value) => *value == *other_value,
                Unbounded::Infinite => false,
            },
            Unbounded::Infinite => match other {
                Unbounded::Value(_) => false,
                Unbounded::Infinite => true,
            },
        }
    }
}

pub type UnboundedSize = Unbounded<usize>;

/// Area an unbounded horizontal and vertical value.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct UnboundedArea {
    pub horizontal: UnboundedSize,
    pub vertical: UnboundedSize,
}

impl UnboundedArea {
    pub fn scroll_left(&mut self, delta: usize) {
        self.horizontal = self.horizontal.sub(delta);
    }

    pub fn scroll_right(&mut self, delta: usize) {
        self.horizontal = self.horizontal.add(delta);
    }

    pub fn _scroll_up(&mut self, delta: usize) {
        self.vertical = self.vertical.sub(delta);
    }

    pub fn _scroll_down(&mut self, delta: usize) {
        self.vertical = self.vertical.add(delta);
    }

    pub fn set_horizontal(&mut self, horizontal: usize) {
        self.horizontal = UnboundedSize::Value(horizontal);
    }

    pub fn set_vertical(&mut self, vertical: usize) {
        self.vertical = UnboundedSize::Value(vertical);
    }

    /// Replace infinite values by integer or keep the current ones if finite.
    pub fn set_bounds(&mut self, horizontal: usize, vertical: usize) -> (usize, usize) {
        (
            match self.horizontal {
                UnboundedSize::Infinite => {
                    self.set_horizontal(horizontal);
                    horizontal
                }
                UnboundedSize::Value(horizontal) => horizontal,
            },
            match self.vertical {
                UnboundedSize::Infinite => {
                    self.set_vertical(vertical);
                    vertical
                }
                UnboundedSize::Value(vertical) => vertical,
            },
        )
    }

    pub fn horizontal_home(&mut self) {
        self.horizontal = UnboundedSize::ZERO;
    }

    pub fn horizontal_end(&mut self) {
        self.horizontal = UnboundedSize::Infinite;
    }
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

/// FIFO with a bounded size.
pub struct BoundedFifo<T>(VecDeque<T>);

impl<T> BoundedFifo<T> {
    pub fn new(capacity: usize) -> Self {
        Self(VecDeque::with_capacity(capacity))
    }

    pub fn capacity(&self) -> usize {
        let Self(v) = self;
        v.capacity()
    }

    pub fn len(&self) -> usize {
        let Self(v) = self;
        v.len()
    }

    pub fn push(&mut self, item: T) {
        let Self(v) = self;
        if v.len() == v.capacity() {
            let _ = v.pop_front();
        }
        v.push_back(item);
    }

    pub fn back(&self) -> Option<&T> {
        let Self(v) = self;
        v.back()
    }

    pub fn front(&self) -> Option<&T> {
        let Self(v) = self;
        v.front()
    }
}

#[cfg(test)]
mod test {

    use super::{BoundedFifo, UnboundedSize};

    #[test]
    fn test_add() {
        assert_eq!(UnboundedSize::Value(7), UnboundedSize::Value(4).add(3));
        assert_eq!(UnboundedSize::Infinite, UnboundedSize::Infinite.add(3));
    }

    #[test]
    fn test_sub() {
        assert_eq!(UnboundedSize::Value(4), UnboundedSize::Value(7).sub(3));
        assert_eq!(UnboundedSize::Value(0), UnboundedSize::Value(3).sub(7));
        assert_eq!(UnboundedSize::Infinite, UnboundedSize::Infinite.sub(7));
    }

    #[test]
    fn test_fifo() {
        let mut v = BoundedFifo::new(2);
        v.push(1usize);
        assert_eq!(1, v.len());
        assert_eq!(1, *v.front().unwrap());
        v.push(2usize);
        assert_eq!(2, v.len());
        assert_eq!(1, *v.front().unwrap());
        v.push(3usize);
        assert_eq!(2, v.len());
        assert_eq!(2, *v.front().unwrap());
    }
}
