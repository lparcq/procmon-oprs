use crate::collector::Collector;
use crate::format::Formatter;

pub mod term;
pub mod text;

pub trait Output {
    fn open(&mut self, collector: &dyn Collector) -> anyhow::Result<()>;
    fn close(&mut self) -> anyhow::Result<()>;
    fn render(
        &mut self,
        collector: &dyn Collector,
        formatters: &Vec<Formatter>,
        targets_updated: bool,
    ) -> anyhow::Result<()>;
    fn pause(&mut self) -> anyhow::Result<bool>;
}

pub use crate::output::term::TerminalOutput;
pub use crate::output::text::TextOutput;
