use super::widget::{Size, Widget};
use std::io::{Result, Write};
use termion::{
    color,
    cursor::Goto,
    event::{Event, Key},
    style,
};

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
                Event::Key(Key::PageUp) => Action::MultiplyTimeout(2),
                Event::Key(Key::PageDown) => Action::DivideTimeout(2),
                _ => Action::None,
            },
        }
    }
}

impl Widget for MenuBar {
    fn write(&self, out: &mut dyn Write, pos: Goto, size: Size) -> Result<()> {
        let (mut remaining_width, _) = size;
        write!(
            out,
            "{}{}{}",
            pos,
            color::Fg(color::White),
            color::Bg(color::Black)
        )?;
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
