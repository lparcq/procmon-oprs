use std::io::{Result, Write};
use termion::cursor::Goto;

pub type Size = (u16, u16);

pub trait Widget {
    fn write(&self, out: &mut dyn Write, pos: Goto, size: Size) -> Result<()>;
}
