// Oprs -- process monitor for Linux
// Copyright (C) 2024 Laurent Pelecq
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
mod managers;
mod metrics;
mod stat;
mod targets;

#[cfg(test)]
mod mocks;

pub mod format;
pub mod parsers;

pub(crate) use self::agg::{Aggregation, AggregationSet};
pub(crate) use self::collector::{Collector, LimitKind, ProcessIdentity, ProcessSamples, Sample};
pub(crate) use self::forest::{Forest, Process, ProcessError, ProcessInfo};
pub(crate) use self::managers::{
    FlatProcessManager, ForestProcessManager, ProcessDetails, ProcessFilter, ProcessManager,
};
pub(crate) use self::metrics::{FormattedMetric, MetricDataType, MetricId, MetricNamesParser};
pub(crate) use self::stat::{Limit, LimitValue, ProcessStat, SystemConf, SystemStat};
pub(crate) use self::targets::{TargetContainer, TargetError, TargetId};
