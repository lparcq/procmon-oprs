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
use crate::collector::{Collector, GridCollector};
use crate::format::Formatter;
use crate::info::SystemConf;
use crate::metrics::MetricId;
use crate::targets::{TargetContainer, TargetId};

mod input;
mod menu;
mod table;
mod widget;

/// Print on standard output as a table
pub struct TerminalOutput<'a> {
    targets: TargetContainer<'a>,
    collector: GridCollector,
    formatters: Vec<Formatter>,
}

impl<'a> TerminalOutput<'a> {
    pub fn new(
        target_ids: &[TargetId],
        metric_ids: &[MetricId],
        formatters: &[Formatter],
        system_conf: &'a SystemConf,
    ) -> anyhow::Result<TerminalOutput<'a>> {
        let mut targets = TargetContainer::new(system_conf);
        targets.push_all(target_ids)?;
        let collector = GridCollector::new(target_ids.len(), metric_ids.to_vec());
        Ok(TerminalOutput {
            targets,
            collector,
            formatters: formatters.to_vec(),
        })
    }

    pub fn is_available() -> bool {
        termion::is_tty(&io::stdin())
    }
}

impl<'a> Output for TerminalOutput<'a> {
    fn run(&mut self, every: Duration, count: Option<u64>) -> anyhow::Result<()> {
        let mut loop_number: u64 = 0;
        let metric_ids = self.collector.metric_ids().clone();

        let mut initial_timeout = every;
        let mut timeout = initial_timeout;

        let mut screen = AlternateScreen::from(MouseTerminal::from(io::stdout().into_raw_mode()?));

        let in_events = input::EventChannel::new();
        let mut menu = MenuBar::new();
        let mut table = TableWidget::new();
        table.set_vertical_header(metric_ids.iter().map(|s| s.to_str().to_string()));

        loop {
            let _ = self.targets.refresh();
            self.targets.collect(&mut self.collector);
            let lines = self.collector.lines();

            // Rendering
            let screen_size = terminal_size()?;
            let (screen_width, screen_height) = screen_size;
            table.clear_columns();
            table.clear_horizontal_header();
            table.append_horizontal_header(lines.iter().map(|line| line.name.to_string()));
            table.append_horizontal_header(lines.iter().map(|line| format!("{}", line.pid,)));
            lines.iter().enumerate().for_each(|(col_num, line)| {
                table.set_column(
                    col_num,
                    self.formatters
                        .iter()
                        .zip(line.metrics.iter())
                        .map(|(fmt, value)| fmt(*value)),
                )
            });
            write!(screen, "{}", clear::All)?;
            table.write(&mut screen, Goto(1, 1), screen_size)?;
            menu.write(&mut screen, Goto(1, screen_height), (screen_width, 1))?;
            write!(screen, "{}", cursor::Hide)?;
            screen.flush()?;

            let stop_watch = Instant::now();
            match in_events.receive_timeout(timeout)? {
                Some(evt) => {
                    match timeout.checked_sub(stop_watch.elapsed()) {
                        Some(rest) => timeout = rest,
                        None => timeout = initial_timeout,
                    }
                    match menu.action(&evt) {
                        Action::Quit => break,
                        Action::MultiplyTimeout(factor) => {
                            if let Some(new_timeout) = initial_timeout.checked_mul(factor as u32) {
                                initial_timeout = new_timeout;
                            }
                        }
                        Action::DivideTimeout(factor) => {
                            if let Some(new_timeout) = initial_timeout.checked_div(factor as u32) {
                                initial_timeout = new_timeout;
                            }
                        }
                        _ => {}
                    }
                }
                None => {
                    timeout = initial_timeout;
                }
            }

            if let Some(count) = count {
                loop_number += 1;
                if loop_number >= count {
                    break;
                }
            }
        }
        write!(screen, "{}", cursor::Show)?;
        screen.flush()?;
        Ok(())
    }
}
