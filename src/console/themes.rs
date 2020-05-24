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

use std::fmt::Display;
use std::io;
use termion::color;

pub enum ColorUse {
    BgShade,
}

/// A list of colors by usage
pub struct Theme {
    colors: [Box<dyn Display>; 1],
}

impl Theme {
    pub fn write_color(&self, out: &mut dyn io::Write, usage: ColorUse) -> io::Result<()> {
        write!(
            out,
            "{}",
            match usage {
                ColorUse::BgShade => &self.colors[0],
            }
        )
    }
}

/// Theme with dark colors
pub fn dark() -> Theme {
    Theme {
        colors: [Box::new(color::Bg(color::AnsiValue::grayscale(3)))],
    }
}

/// Theme with light colors
pub fn light() -> Theme {
    Theme {
        colors: [Box::new(color::Bg(color::AnsiValue::grayscale(22)))],
    }
}