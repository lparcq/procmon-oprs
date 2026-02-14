// Oprs -- process monitor for Linux
// Copyright (C) 2024-2026 Laurent Pelecq
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

mod agg;
mod collector;
mod forest;
mod interpreters;
mod managers;
mod metrics;
mod stat;
mod targets;

#[cfg(test)]
mod mocks;

pub mod format;
pub mod matchers;
pub mod parsers;

pub(crate) use self::agg::{Aggregation, AggregationSet};

#[cfg(feature = "tui")]
pub(crate) use self::collector::Sample;
pub(crate) use self::collector::{Collector, ProcessIdentity, ProcessSamples};

#[cfg(feature = "tui")]
pub(crate) use self::forest::format_result;
pub(crate) use self::forest::{Forest, Process, ProcessError, ProcessInfo, ProcessResult, Signal};

pub(crate) use self::managers::{FlatProcessManager, ForestProcessManager, ProcessManager};
#[cfg(feature = "tui")]
pub(crate) use self::managers::{ProcessDetails, ProcessFilter};

pub(crate) use self::metrics::{
    FormattedMetric, MetricDataType, MetricFormat, MetricId, MetricNamesParser,
};
pub(crate) use self::stat::{ProcessStat, SystemConf, SystemStat};
pub(crate) use self::targets::{TargetContainer, TargetError, TargetId};
