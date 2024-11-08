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

use std::slice::Iter as SliceIter;

use crate::{clock::Timer, collector::Collector, metrics::FormattedMetric};

pub mod null;
pub mod term;
pub mod text;

mod types;

pub enum PauseStatus {
    Quit,
    TimeOut,
    Interrupted,
}

pub trait DisplayDevice {
    /// Initialize the device with the metrics.
    fn open(&mut self, metrics: SliceIter<FormattedMetric>) -> anyhow::Result<()>;

    /// Close the device.
    fn close(&mut self) -> anyhow::Result<()>;

    /// Render the metrics on the device.
    fn render(&mut self, collector: &Collector, targets_updated: bool) -> anyhow::Result<()>;

    /// Pause for the given duration.
    fn pause(&mut self, _: &mut Timer) -> anyhow::Result<PauseStatus> {
        panic!("not available");
    }
}

pub use crate::display::null::NullDevice;
pub use crate::display::term::TerminalDevice;
pub use crate::display::text::TextDevice;
