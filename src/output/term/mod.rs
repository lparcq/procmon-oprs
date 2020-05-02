use std::cmp;
use std::io;
use std::time::{Duration, Instant};
use termion::{
    event::{Event, Key},
    input::MouseTerminal,
    raw::IntoRawMode,
    screen::AlternateScreen,
};
use tui::{
    backend::TermionBackend,
    layout::{Alignment, Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph, Row, Table, TableState, Text},
    Terminal,
};

use super::Output;
use crate::collector::{Collector, GridCollector};
use crate::format::Formatter;
use crate::info::SystemConf;
use crate::metric::MetricId;
use crate::targets::{TargetContainer, TargetId};

mod input;

/// Print on standard output as a table
pub struct TerminalOutput<'a> {
    targets: TargetContainer<'a>,
    collector: GridCollector,
    formatters: Vec<Formatter>,
}

impl<'a> TerminalOutput<'a> {
    pub fn new(
        target_ids: &[TargetId],
        metric_ids: Vec<MetricId>,
        formatters: Vec<Formatter>,
        system_conf: &'a SystemConf,
    ) -> anyhow::Result<TerminalOutput<'a>> {
        let mut targets = TargetContainer::new(system_conf);
        targets.push_all(target_ids)?;
        let collector = GridCollector::new(target_ids.len(), metric_ids);
        Ok(TerminalOutput {
            targets,
            collector,
            formatters,
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

        let in_events = input::EventChannel::new();
        let mut initial_timeout = every;
        let mut timeout = initial_timeout;

        let stdout = io::stdout().into_raw_mode()?;
        let stdout = MouseTerminal::from(stdout);
        let stdout = AlternateScreen::from(stdout);
        let backend = TermionBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.hide_cursor()?;

        let mut table_state = TableState::default();
        let top_left = vec![String::new()];

        let left_column_width = metric_ids
            .iter()
            .fold(0, |width, metric| cmp::max(width, metric.to_str().len()));

        let mut message = format!("Esc to exit");
        loop {
            let _ = self.targets.refresh();
            self.targets.collect(&mut self.collector);
            let lines = self.collector.lines();
            terminal.draw(|mut frame| {
                let mut widths = Vec::new();
                widths.push(Constraint::Min(left_column_width as u16));
                (0..lines.len()).for_each(|_| widths.push(Constraint::Min(10)));
                message = format!(
                    "Esc to exit: #{} {}",
                    lines.len(),
                    lines
                        .iter()
                        .map(|line| match &line.metrics {
                            Some(metrics) => format!("{} [{}]", line.name, metrics.pid,),
                            None => line.name.to_string(),
                        })
                        .collect::<String>()
                );
                let header_iter =
                    top_left
                        .iter()
                        .map(|s| s.clone())
                        .chain(lines.iter().map(|line| match &line.metrics {
                            Some(metrics) => format!("{} [{}]", line.name, metrics.pid,),
                            None => line.name.to_string(),
                        }));
                let normal_style = Style::default().fg(Color::White);
                let rows_iter = self
                    .formatters
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(index, fmt)| {
                        let row_iter = metric_ids
                            .iter()
                            .skip(index)
                            .take(1)
                            .map(|m| m.to_str().to_string())
                            .chain(lines.iter().map(move |line| match &line.metrics {
                                Some(metrics) => fmt(metrics.series[index]),
                                None => String::new(),
                            }));
                        Row::StyledData(row_iter, normal_style)
                    });

                // Hack: it's not possible to write the last line if the rect height is one.
                // Using height 2 with a newline.
                let mut message_height = 0;
                let mut extra_height = 0;
                let mut message_lines: Vec<Text> = message
                    .lines()
                    .map(|line| {
                        message_height += 1;
                        Text::raw(format!("{}\n", line))
                    })
                    .collect();
                if message_height == 1 {
                    message_height += 1;
                    extra_height = 1;
                    message_lines.insert(0, Text::raw("\n"));
                }

                let frame_rect = frame.size();
                let frame_width = frame_rect.right() - frame_rect.left();
                let frame_height = frame_rect.bottom() - frame_rect.top();
                let rects = [
                    Rect::new(
                        frame_rect.left(),
                        frame_rect.top(),
                        frame_width,
                        frame_height - message_height + extra_height,
                    ),
                    Rect::new(
                        frame_rect.left(),
                        frame_rect.bottom() - message_height,
                        frame_width,
                        message_height,
                    ),
                ];

                let selected_style = Style::default().fg(Color::Yellow).modifier(Modifier::BOLD);
                let table = Table::new(header_iter, rows_iter)
                    .block(Block::default().borders(Borders::ALL))
                    .highlight_style(selected_style)
                    .highlight_symbol(">> ")
                    .widths(&widths);
                frame.render_stateful_widget(table, rects[0], &mut table_state);

                let paragraph = Paragraph::new(message_lines.iter())
                    .block(Block::default().borders(Borders::NONE))
                    .alignment(Alignment::Left)
                    .wrap(false);
                frame.render_widget(paragraph, rects[1]);
            })?;

            let stop_watch = Instant::now();
            match in_events.receive_timeout(timeout)? {
                Some(evt) => {
                    match timeout.checked_sub(stop_watch.elapsed()) {
                        Some(rest) => timeout = rest,
                        None => timeout = initial_timeout,
                    }
                    match evt {
                        Event::Key(Key::Esc) => break,
                        Event::Key(Key::PageUp) => {
                            if let Some(new_timeout) = initial_timeout.checked_mul(2) {
                                initial_timeout = new_timeout;
                            }
                        }
                        Event::Key(Key::PageDown) => {
                            if let Some(new_timeout) = initial_timeout.checked_div(2) {
                                initial_timeout = new_timeout;
                            }
                        }
                        Event::Key(Key::Char(c)) => {
                            message = format!("got {}", c);
                        }
                        // Event::Mouse(me) => match me {
                        //     MouseEvent::Press(_, x, y) => {
                        //     }
                        //     _ => (),
                        // },
                        _ => {
                            message = format!("something else {:?}", evt);
                        }
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
        Ok(())
    }
}
