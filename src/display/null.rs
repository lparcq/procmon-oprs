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

use crate::{collector::Collector, metrics::FormattedMetric};

use super::{DisplayDevice, SliceIter};

/// Null device
pub struct NullDevice {}

impl NullDevice {
    pub fn new() -> Self {
        Self {}
    }
}

impl DisplayDevice for NullDevice {
    fn open(&mut self, _metrics: SliceIter<FormattedMetric>) -> anyhow::Result<()> {
        Ok(())
    }

    fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn render(&mut self, _collector: &Collector, _targets_updated: bool) -> anyhow::Result<()> {
        Ok(())
    }
}
