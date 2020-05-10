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
use crate::collector::Collector;
use crate::format::Formatter;
use crate::metrics::MetricId;

mod input;
mod menu;
mod table;
mod widget;

/// Print on standard output as a table
pub struct TerminalOutput {
    every: Duration,
    metric_ids: Vec<MetricId>,
    events: input::EventChannel,
    screen: Box<dyn Write>,
    menu: MenuBar,
    table: TableWidget,
}

impl TerminalOutput {
    pub fn new(every: Duration) -> anyhow::Result<TerminalOutput> {
        Ok(TerminalOutput {
            every,
            metric_ids: Vec::new(),
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
    fn open(&mut self, collector: &dyn Collector) -> anyhow::Result<()> {
        self.metric_ids.extend(collector.metric_ids());
        self.table
            .set_vertical_header(self.metric_ids.iter().map(|s| s.to_str().to_string()));
        Ok(())
    }

    fn close(&mut self) -> anyhow::Result<()> {
        write!(self.screen, "{}", cursor::Show)?;
        self.screen.flush()?;
        Ok(())
    }

    fn render(
        &mut self,
        collector: &dyn Collector,
        formatters: &Vec<Formatter>,
        _targets_updated: bool,
    ) -> anyhow::Result<()> {
        let lines = collector.lines();

        let screen_size = terminal_size()?;
        let (screen_width, screen_height) = screen_size;
        self.table.clear_columns();
        self.table.clear_horizontal_header();
        self.table
            .append_horizontal_header(lines.iter().map(|line| line.name.to_string()));
        self.table
            .append_horizontal_header(lines.iter().map(|line| format!("{}", line.pid,)));
        lines.iter().enumerate().for_each(|(col_num, line)| {
            self.table.set_column(
                col_num,
                formatters
                    .iter()
                    .zip(line.metrics.iter())
                    .map(|(fmt, value)| fmt(*value)),
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
        loop {
            match self.events.receive_timeout(timeout)? {
                Some(evt) => {
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
                None => {
                    break;
                }
            }
        }
        Ok(true)
    }
}
