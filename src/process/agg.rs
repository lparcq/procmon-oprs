// Oprs -- process monitor for Linux
// Copyright (C) 2020-2025  Laurent Pelecq
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
use strum::{EnumIter, EnumString};

/// Possible metric aggregations
#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumIter, EnumString)]
pub enum Aggregation {
    #[strum(serialize = "raw")]
    None,
    #[strum(serialize = "min")]
    Min,
    #[strum(serialize = "max")]
    Max,
    #[strum(serialize = "ratio")]
    Ratio,
}

impl Aggregation {
    fn mask(self) -> u8 {
        match self {
            Aggregation::None => 0x01,
            Aggregation::Min => 0x02,
            Aggregation::Max => 0x04,
            Aggregation::Ratio => 0x08,
        }
    }
}

/// Partial Ordering
impl cmp::PartialOrd for Aggregation {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Ordering
impl cmp::Ord for Aggregation {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.mask().cmp(&other.mask())
    }
}

/// A set of aggregations
#[derive(Clone, Copy, Debug)]
pub struct AggregationSet(u8);

impl AggregationSet {
    pub fn new() -> AggregationSet {
        AggregationSet(0)
    }

    pub fn has(self, variant: Aggregation) -> bool {
        self.0 & variant.mask() != 0
    }

    pub fn set(&mut self, variant: Aggregation) {
        self.0 |= variant.mask();
    }
}

#[cfg(test)]
mod tests {

    use super::{Aggregation, AggregationSet};
    use strum::IntoEnumIterator;

    #[test]
    fn test_each_aggregation() {
        for variant in Aggregation::iter() {
            let mut aggs = AggregationSet::new();
            assert!(!aggs.has(variant), "{variant:?}: should not be set");
            aggs.set(variant);
            assert!(aggs.has(variant), "{variant:?}: is not set");
            Aggregation::iter().for_each(|other| {
                assert!(
                    other.mask() == variant.mask() || !aggs.has(other),
                    "{other:?}: only {variant:?} shoult be set",
                );
            });
        }
    }

    #[test]
    fn test_multiples() {
        let mut aggs = AggregationSet::new();
        aggs.set(Aggregation::Min);
        aggs.set(Aggregation::Ratio);
        assert!(aggs.has(Aggregation::Min));
        assert!(aggs.has(Aggregation::Ratio));
        assert!(!aggs.has(Aggregation::None));
        assert!(!aggs.has(Aggregation::Max));
    }
}
