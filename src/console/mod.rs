// Oprs -- process monitor for Linux
// Copyright (C) 2020, 2021  Laurent Pelecq
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

use strum_macros::EnumString;

pub use self::input::{is_tty, Event, EventChannel, Key};

pub mod charset;

mod input;

#[derive(Clone, Copy, Debug, EnumString, PartialEq, Eq)]
pub enum BuiltinTheme {
    #[strum(serialize = "light")]
    Light,
    #[strum(serialize = "dark")]
    Dark,
    #[strum(serialize = "light16")]
    Light16,
    #[strum(serialize = "dark16")]
    Dark16,
}
