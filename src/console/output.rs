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

use std::io::{self, Write};
use termion::{
    clear, color,
    cursor::{self, Goto},
    raw::IntoRawMode,
    screen::AlternateScreen,
    style, terminal_size,
};

use super::themes::{ColorUse, Theme};

#[derive(Clone, Copy)]
pub enum BuiltinTheme {
    Dark,
    Light,
}

/// A starting point to write
#[derive(Clone, Copy)]
pub struct Origin(pub u16, pub u16);

impl Origin {
    /// Get x
    pub fn get_x(self) -> u16 {
        let Origin(x, _) = self;
        x
    }

    /// Replace x in origin
    pub fn with_x(self, x: u16) -> Origin {
        let Origin(_, y) = self;
        Origin(x, y)
    }
}

/// Rectangle area size (width, height)
pub struct Size(pub u16, pub u16);

/// Rectangle area position and size (x, y, width, height)
pub struct Clip(pub u16, pub u16, pub u16, pub u16);

/// Terminal screen
pub struct Screen {
    out: Box<dyn Write>,
    theme: Option<Theme>,
}

impl Screen {
    pub fn new() -> anyhow::Result<Screen> {
        Ok(Screen {
            out: Box::new(AlternateScreen::from(io::stdout().into_raw_mode()?)),
            theme: None,
        })
    }

    /// Total size of the screen
    pub fn size(&self) -> io::Result<Size> {
        let (width, height) = terminal_size()?;
        Ok(Size(width, height))
    }

    /// Show the cursor
    pub fn cursor_show(&mut self) -> io::Result<&mut Self> {
        write!(self.out, "{}", cursor::Show)?;
        Ok(self)
    }

    /// Hide the cursor
    pub fn cursor_hide(&mut self) -> io::Result<&mut Self> {
        write!(self.out, "{}", cursor::Hide)?;
        Ok(self)
    }

    /// Clear the screen
    pub fn clear_all(&mut self) -> io::Result<&mut Self> {
        write!(self.out, "{}", clear::All)?;
        Ok(self)
    }

    /// Go to a given position on the screen
    pub fn goto(&mut self, x: u16, y: u16) -> io::Result<&mut Self> {
        write!(self.out, "{}", Goto(x, y))?;
        Ok(self)
    }

    /// Go to a given origin on the screen
    pub fn origin(&mut self, origin: Origin) -> io::Result<&mut Self> {
        let Origin(x, y) = origin;
        self.goto(x, y)
    }

    /// Enable bold
    pub fn bold(&mut self) -> io::Result<&mut Self> {
        write!(self.out, "{}", style::Bold)?;
        Ok(self)
    }

    /// Invert foreground and background
    pub fn invert(&mut self) -> io::Result<&mut Self> {
        write!(self.out, "{}", style::Invert)?;
        Ok(self)
    }

    /// Reset style to default
    pub fn style_reset(&mut self) -> io::Result<&mut Self> {
        write!(self.out, "{}", style::Reset)?;
        Ok(self)
    }

    /// Set color theme
    pub fn set_theme(&mut self, theme: BuiltinTheme) {
        self.theme = Some(match theme {
            BuiltinTheme::Dark => super::themes::dark(),
            BuiltinTheme::Light => super::themes::light(),
        });
    }

    /// Reset color to default
    pub fn bg_reset(&mut self) -> io::Result<&mut Self> {
        write!(self.out, "{}", color::Bg(color::Reset))?;
        Ok(self)
    }

    /// Background shade
    pub fn bg_shade(&mut self) -> io::Result<&mut Self> {
        if let Some(theme) = &self.theme {
            theme.write_color(&mut self.out, ColorUse::BgShade)?;
        }
        Ok(self)
    }
}

impl Write for Screen {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.out.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}
