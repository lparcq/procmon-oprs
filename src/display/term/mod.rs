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

use std::io;

pub use self::device::TerminalDevice;

use crate::console::{Clip, Screen};

mod device;
mod menu;
mod sizer;
mod table;

const ELASTICITY: usize = 2;
const BORDER_WIDTH: usize = 1;
const MENU_HEIGHT: usize = 1;
const HEADER_HEIGHT: usize = 2;

trait Widget {
    fn write(&self, screen: &mut Screen, clip: &Clip) -> io::Result<()>;
}
