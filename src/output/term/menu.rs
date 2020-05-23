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

use super::Widget;
use crate::console::{Clip, Event, Key, Screen};

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
        screen: &mut Screen,
        key: Key,
        name: &str,
        separator: Option<&str>,
        remaining_width: &mut u16,
    ) -> Result<()> {
        if (*remaining_width as usize) < name.len() + 4 {
            // Not sure to have space
            return Ok(());
        }

        if let Some(separator) = separator {
            write!(screen, "{}", separator)?;
        }
        screen.invert()?;
        let mut width = 0;
        match key {
            Key::Backspace => write_len!(screen, "⌫", width)?,
            Key::Left => write_len!(screen, "⇲", width)?,
            Key::Right => write_len!(screen, "⬅", width)?,
            Key::Up => write_len!(screen, "⬆", width)?,
            Key::Down => write_len!(screen, "⬇", width)?,
            Key::Home => write_len!(screen, "⇱", width)?,
            Key::End => write_len!(screen, "⇲", width)?,
            Key::PageUp => write_len!(screen, "PgUp", width)?,
            Key::PageDown => write_len!(screen, "PgDn", width)?,
            Key::BackTab => write_len!(screen, "⇤", width)?,
            Key::Delete => write_len!(screen, "⌧", width)?,
            Key::Insert => write_len!(screen, "Ins", width)?,
            Key::F(num) => write_len!(screen, format!("F{}", num), width)?,
            Key::Char('\t') => write_len!(screen, "⇥", width)?,
            Key::Char(ch) => write_len!(screen, format!("{}", ch), width)?,
            Key::Alt(ch) => write_len!(screen, format!("M-{}", ch), width)?,
            Key::Ctrl(ch) => write_len!(screen, format!("C-{}", ch), width)?,
            Key::Null => write_len!(screen, "\\0", width)?,
            Key::Esc => write_len!(screen, "Esc", width)?,
            _ => write_len!(screen, "?", width)?,
        };
        screen.reset()?;
        write!(screen, " {}", name)?;
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
    fn write(&self, screen: &mut Screen, clip: &Clip) -> Result<()> {
        let Clip(x, y, mut remaining_width, _) = *clip;
        screen.goto(x, y)?;
        match self.context {
            MenuContext::Root => {
                self.write_entry(screen, Key::Esc, "Quit", None, &mut remaining_width)?;
                self.write_entry(
                    screen,
                    Key::PageUp,
                    "Faster",
                    Some(" "),
                    &mut remaining_width,
                )?;
                self.write_entry(
                    screen,
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
