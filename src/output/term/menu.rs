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

use std::io::{Result, Write};
use termion::{
    cursor::Goto,
    event::{Event, Key},
    style,
};

use super::{ScreenSize, Widget};

macro_rules! write_len {
    ($out:expr, $s:expr, $len:expr) => {{
        let val = $s;
        let res = write!($out, "{}", val);
        $len += val.len();
        res
    }};
}

/// Action
pub enum Action {
    None,
    Quit,
    MultiplyTimeout(u16),
    DivideTimeout(u16),
    ScrollRight,
    ScrollUp,
    ScrollDown,
    ScrollLeft,
}

/// Menu context
enum MenuContext {
    Root,
}

/// Menubar
pub struct MenuBar {
    context: MenuContext,
}

impl MenuBar {
    pub fn new() -> MenuBar {
        MenuBar {
            context: MenuContext::Root,
        }
    }

    fn write_entry(
        &self,
        out: &mut dyn Write,
        key: Key,
        name: &str,
        separator: Option<&str>,
        remaining_width: &mut u16,
    ) -> Result<()> {
        if (*remaining_width as usize) < name.len() + 4 {
            // Not sure to have space
            return Ok(());
        }
        write!(out, "{}{}", separator.unwrap_or(""), style::Invert)?;
        let mut width = 0;
        match key {
            Key::Backspace => write_len!(out, "⌫", width)?,
            Key::Left => write_len!(out, "⇲", width)?,
            Key::Right => write_len!(out, "⬅", width)?,
            Key::Up => write_len!(out, "⬆", width)?,
            Key::Down => write_len!(out, "⬇", width)?,
            Key::Home => write_len!(out, "⇱", width)?,
            Key::End => write_len!(out, "⇲", width)?,
            Key::PageUp => write_len!(out, "PgUp", width)?,
            Key::PageDown => write_len!(out, "PgDn", width)?,
            Key::BackTab => write_len!(out, "⇤", width)?,
            Key::Delete => write_len!(out, "⌧", width)?,
            Key::Insert => write_len!(out, "Ins", width)?,
            Key::F(num) => write_len!(out, format!("F{}", num), width)?,
            Key::Char('\t') => write_len!(out, "⇥", width)?,
            Key::Char(ch) => write_len!(out, format!("{}", ch), width)?,
            Key::Alt(ch) => write_len!(out, format!("M-{}", ch), width)?,
            Key::Ctrl(ch) => write_len!(out, format!("C-{}", ch), width)?,
            Key::Null => write_len!(out, "\\0", width)?,
            Key::Esc => write_len!(out, "Esc", width)?,
            _ => write_len!(out, "?", width)?,
        };
        write!(out, "{} {}", style::Reset, name)?;
        *remaining_width -= (width + name.len() + 1) as u16;
        Ok(())
    }

    pub fn action(&mut self, evt: &Event) -> Action {
        match self.context {
            MenuContext::Root => match evt {
                Event::Key(Key::Esc) => Action::Quit,
                Event::Key(Key::PageUp) => Action::DivideTimeout(2),
                Event::Key(Key::PageDown) => Action::MultiplyTimeout(2),
                Event::Key(Key::Right) => Action::ScrollRight,
                Event::Key(Key::Up) => Action::ScrollUp,
                Event::Key(Key::Down) => Action::ScrollDown,
                Event::Key(Key::Left) => Action::ScrollLeft,
                _ => Action::None,
            },
        }
    }
}

impl Widget for MenuBar {
    fn write(&self, out: &mut dyn Write, pos: Goto, size: ScreenSize) -> Result<()> {
        let (mut remaining_width, _) = size;
        write!(out, "{}", pos,)?;
        match self.context {
            MenuContext::Root => {
                self.write_entry(out, Key::Esc, "Quit", None, &mut remaining_width)?;
                self.write_entry(out, Key::PageUp, "Faster", Some(" "), &mut remaining_width)?;
                self.write_entry(
                    out,
                    Key::PageDown,
                    "Slower",
                    Some(" "),
                    &mut remaining_width,
                )?;
            }
        }
        Ok(())
    }
}
