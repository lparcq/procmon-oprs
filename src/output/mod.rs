use std::time;

pub mod text;

pub trait Output {
    fn run(&mut self, every_ms: time::Duration, count: Option<u64>);
}

pub use crate::output::text::TextOutput;
