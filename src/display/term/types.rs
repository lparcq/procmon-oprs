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
    ops::{Add, Sub},
};

/// Unsigned integer type with an infinite value.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Unbounded<T: Clone + Copy + Default> {
    Value(T),
    Infinite,
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

pub(crate) type UnboundedSize = Unbounded<usize>;

#[cfg(test)]
mod test {

    use super::UnboundedSize;

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
