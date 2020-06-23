// Oprs -- process monitor for Linux
// Copyright (C) 2020  Laurent Pelecq
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

use crate::clock::Timer;
use crate::collector::Collector;

pub mod term;
pub mod text;

pub enum PauseStatus {
    Quit,
    TimeOut,
    Interrupted,
}

pub trait DisplayDevice {
    fn open(&mut self, collector: &Collector) -> anyhow::Result<()>;

    fn close(&mut self) -> anyhow::Result<()>;

    fn render(&mut self, collector: &Collector, targets_updated: bool) -> anyhow::Result<()>;

    fn is_interactive(&self) -> bool {
        false
    }

    fn pause(&mut self, _: &mut Timer) -> anyhow::Result<PauseStatus> {
        panic!("not available");
    }
}

pub use crate::display::term::TerminalDevice;
pub use crate::display::text::TextDevice;
