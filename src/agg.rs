// Support for aggregation of metrics

use std::cmp;
use strum_macros::{EnumIter, EnumString};

/// Possible metric aggregations
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, EnumIter, EnumString)]
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
            assert!(!aggs.has(variant), "{:?}: should not be set", variant);
            aggs.set(variant);
            assert!(aggs.has(variant), "{:?}: is not set", variant);
            Aggregation::iter().for_each(|other| {
                assert!(
                    other.mask() == variant.mask() || !aggs.has(other),
                    "{:?}: only {:?} shoult be set",
                    other,
                    variant
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
