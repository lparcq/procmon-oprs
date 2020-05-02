use std::time;

pub mod term;
pub mod text;

pub trait Output {
    fn run(&mut self, every_ms: time::Duration, count: Option<u64>) -> anyhow::Result<()>;
}

pub use crate::output::term::TerminalOutput;
pub use crate::output::text::TextOutput;
