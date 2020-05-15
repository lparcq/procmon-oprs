use std::io::{self, Write};
use std::time::{Duration, Instant};
use termion::{
    clear,
    cursor::{self, Goto},
    input::MouseTerminal,
    raw::IntoRawMode,
    screen::AlternateScreen,
    terminal_size,
};

use self::menu::{Action, MenuBar};
use self::table::TableWidget;
use self::widget::Widget;
use super::Output;
use crate::agg::Aggregation;
use crate::collector::Collector;

mod input;
mod menu;
mod table;
mod widget;

/// Print on standard output as a table
pub struct TerminalOutput {
    every: Duration,
    events: input::EventChannel,
    screen: Box<dyn Write>,
    menu: MenuBar,
    table: TableWidget,
}

impl TerminalOutput {
    pub fn new(every: Duration) -> anyhow::Result<TerminalOutput> {
        Ok(TerminalOutput {
            every,
            events: input::EventChannel::new(),
            screen: Box::new(AlternateScreen::from(MouseTerminal::from(
                io::stdout().into_raw_mode()?,
            ))),
            menu: MenuBar::new(),
            table: TableWidget::new(),
        })
    }

    pub fn is_available() -> bool {
        termion::is_tty(&io::stdin())
    }
}

impl Output for TerminalOutput {
    fn open(&mut self, collector: &Collector) -> anyhow::Result<()> {
        let mut names = Vec::new();
        let mut last_id = None;
        collector.for_each_computed_metric(|id, ag| {
            if last_id.is_none() || last_id.unwrap() != id {
                last_id = Some(id);
                names.push(id.to_str());
            } else {
                names.push(match ag {
                    Aggregation::None => "none", // never used
                    Aggregation::Min => "min",
                    Aggregation::Max => "max",
                    Aggregation::Ratio => "ratio",
                })
            }
        });
        self.table.set_vertical_header(names);
        Ok(())
    }

    fn close(&mut self) -> anyhow::Result<()> {
        write!(self.screen, "{}", cursor::Show)?;
        self.screen.flush()?;
        Ok(())
    }

    fn render(&mut self, collector: &Collector, _targets_updated: bool) -> anyhow::Result<()> {
        let screen_size = terminal_size()?;
        let (screen_width, screen_height) = screen_size;
        self.table.clear_columns();
        self.table.clear_horizontal_header();
        self.table
            .append_horizontal_header(collector.lines().map(|line| line.get_name().to_string()));
        self.table
            .append_horizontal_header(collector.lines().map(|line| format!("{}", line.get_pid(),)));
        collector.lines().enumerate().for_each(|(col_num, proc)| {
            self.table.set_column(
                col_num,
                proc.samples()
                    .map(|sample| sample.strings())
                    .flatten()
                    .map(|s| s.to_string()),
            )
        });
        write!(self.screen, "{}", clear::All)?;
        self.table
            .write(&mut self.screen, Goto(1, 1), screen_size)?;
        self.menu
            .write(&mut self.screen, Goto(1, screen_height), (screen_width, 1))?;
        write!(self.screen, "{}", cursor::Hide)?;
        self.screen.flush()?;
        Ok(())
    }

    fn pause(&mut self) -> anyhow::Result<bool> {
        let mut timeout = self.every;
        let stop_watch = Instant::now();
        while let Some(evt) = self.events.receive_timeout(timeout)? {
            match timeout.checked_sub(stop_watch.elapsed()) {
                Some(rest) => timeout = rest,
                None => timeout = self.every,
            }
            match self.menu.action(&evt) {
                Action::Quit => return Ok(false),
                Action::MultiplyTimeout(factor) => {
                    if let Some(every) = self.every.checked_mul(factor as u32) {
                        self.every = every;
                    }
                }
                Action::DivideTimeout(factor) => {
                    if let Some(every) = self.every.checked_div(factor as u32) {
                        self.every = every;
                    }
                }
                _ => {}
            }
        }
        Ok(true)
    }
}
