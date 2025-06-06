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

use crate::process::FormattedMetric;

use super::{DisplayDevice, PaneData, PaneKind, SliceIter};

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

    fn render(
        &mut self,
        _pane_kind: PaneKind,
        _data: PaneData,
        _redraw: bool,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
