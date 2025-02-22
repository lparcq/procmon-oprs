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

use std::slice::Iter as SliceIter;

use crate::process::{Collector, FormattedMetric};

#[cfg(feature = "tui")]
use crate::{
    clock::Timer,
    process::{Process, ProcessDetails},
};

pub mod null;
pub mod text;

#[cfg(feature = "tui")]
pub mod term;

/// Status of the device when returning from a pause.
#[cfg(feature = "tui")]
#[derive(Debug)]
pub enum PauseStatus {
    TimeOut,
    Action(Interaction),
}

#[cfg(feature = "tui")]
#[derive(Debug, Clone, Copy)]
pub enum DataKind {
    Details,
    Environment,
    Files,
    Limits,
    _Maps,
    _Threads,
}

#[derive(Debug, Clone, Copy)]
pub enum PaneKind {
    Main,
    #[cfg(feature = "tui")]
    Process(DataKind),
    #[cfg(feature = "tui")]
    Help,
}

/// Data to display the pane.
pub enum PaneData<'a, 'p> {
    /// No data.
    #[cfg(feature = "tui")]
    None,
    /// The collector for all processes.
    Collector(&'p Collector<'a>),
    /// The details for one process.
    #[cfg(feature = "tui")]
    Details(&'p ProcessDetails<'a>),
    /// The process.
    #[cfg(feature = "tui")]
    Process(&'p Process),
}

pub trait DisplayDevice {
    /// Initialize the device with the metrics.
    fn open(&mut self, metrics: SliceIter<FormattedMetric>) -> anyhow::Result<()>;

    /// Close the device.
    fn close(&mut self) -> anyhow::Result<()>;

    /// Render the metrics on the device.
    ///
    /// If `redraw` is true, it is a hint to tell to the device to redraw
    /// entirely the output.
    fn render(&mut self, pane_kind: PaneKind, data: PaneData, redraw: bool) -> anyhow::Result<()>;

    /// Pause for the given duration.
    #[cfg(feature = "tui")]
    fn pause(&mut self, _: &mut Timer) -> anyhow::Result<PauseStatus> {
        panic!("not available");
    }
}

pub use null::NullDevice;
pub use text::TextDevice;

#[cfg(feature = "tui")]
pub use term::{Interaction, TerminalDevice};
