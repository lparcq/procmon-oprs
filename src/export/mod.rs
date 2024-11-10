// Oprs -- process monitor for Linux
// Copyright (C) 2020-2024  Laurent Pelecq
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

use std::{slice::Iter as SliceIter, time::Duration};

use crate::process::{Collector, FormattedMetric};

mod csv;
mod rrd;
mod rrdtool;

pub trait Exporter {
    /// Initialize the exporter with the metrics.
    fn open(&mut self, metrics: SliceIter<FormattedMetric>) -> anyhow::Result<()>;

    /// Terminate exporting.
    fn close(&mut self) -> anyhow::Result<()>;

    /// Export the current metrics.
    fn export(&mut self, collector: &Collector, timestamp: &Duration) -> anyhow::Result<()>;
}

pub use crate::export::{csv::CsvExporter, rrd::RrdExporter};
