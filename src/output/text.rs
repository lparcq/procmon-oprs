use std::thread;
use std::time;

use super::Output;
use crate::collector::{Collector, GridCollector};
use crate::format::Formatter;
use crate::info::SystemConf;
use crate::metric::MetricId;
use crate::targets::{TargetContainer, TargetId};

const REPEAT_HEADER_EVERY: u16 = 20;
const RESIZE_IF_COLUMNS_SHRINK: usize = 2;

fn divide(numerator: usize, denominator: usize) -> (usize, usize) {
    let quotient = numerator / denominator;
    (quotient, numerator - quotient * denominator)
}

/// Table
struct Table {
    titles: Vec<String>,
    subtitles: Vec<String>,
    values: Vec<String>,
    title_width: usize,
    column_width: usize,
    repeat: u16,
}

impl Table {
    fn new() -> Table {
        Table {
            titles: Vec::new(),
            subtitles: Vec::new(),
            values: Vec::new(),
            title_width: 0,
            column_width: 0,
            repeat: 0,
        }
    }

    fn clear_titles(&mut self) {
        self.titles.clear();
    }

    fn push_title(&mut self, title: String) {
        self.titles.push(title);
    }

    fn push_subtitle(&mut self, subtitle: String) {
        self.subtitles.push(subtitle);
    }

    fn clear_values(&mut self) {
        self.values.clear();
    }

    fn push_value(&mut self, value: String) {
        self.values.push(value);
    }

    fn horizontal_rule(&self, column_count: usize, column_width: usize, separator: &str) {
        for _ in 0..column_count {
            print!("{}{:-<width$}", separator, "", width = column_width + 2);
        }
        println!("{}", separator);
    }

    fn print_header(&self) {
        // An horizontal rule
        let title_count = self.titles.len();
        self.horizontal_rule(title_count, self.title_width, "|");
        // Titles
        for title in &self.titles {
            print!("| {:^width$} ", title, width = self.title_width);
        }
        println!("|");
        self.horizontal_rule(title_count, self.title_width, "+");
        // Subtitles
        for _ in 0..title_count {
            for subtitle in &self.subtitles {
                print!("| {:^width$} ", subtitle, width = self.column_width);
            }
        }
        println!("|");
    }

    fn print_values(&self) {
        for value in &self.values {
            print!("| {:^width$} ", value, width = self.column_width);
        }
        println!("|");
    }

    /// Calculate the column width
    fn resize(&mut self) {
        let subtitle_count = self.subtitles.len();
        let mut column_width = 0;
        for title in &self.titles {
            // minimum column with to display the title
            let (quotient, remainder) = divide(title.len() + 3, subtitle_count);
            let min_col_width = quotient - 3 + if remainder > 0 { 1 } else { 0 };
            if min_col_width > column_width {
                column_width = min_col_width;
            }
        }
        for subtitle in &self.subtitles {
            if subtitle.len() > column_width {
                column_width = subtitle.len();
            }
        }
        for value in &self.values {
            if value.len() > column_width {
                column_width = value.len();
            }
        }
        let title_width = (column_width + 3) * subtitle_count - 3;
        if column_width > self.column_width
            || self.column_width - column_width > RESIZE_IF_COLUMNS_SHRINK
        {
            self.column_width = column_width;
            self.title_width = title_width;
            self.repeat = 0;
        }
    }

    fn print(&mut self, with_header: bool) {
        if with_header || self.repeat == 0 {
            self.print_header();
        }
        self.print_values();
        self.repeat += 1;
        if self.repeat >= REPEAT_HEADER_EVERY {
            self.repeat = 0;
        }
    }
}

/// Print on standard output as a table
pub struct TextOutput<'a> {
    targets: TargetContainer<'a>,
    collector: GridCollector,
    formatters: Vec<Formatter>,
}

impl<'a> TextOutput<'a> {
    pub fn new(
        target_ids: &[TargetId],
        metric_ids: Vec<MetricId>,
        formatters: Vec<Formatter>,
        system_conf: &'a SystemConf,
    ) -> anyhow::Result<TextOutput<'a>> {
        let mut targets = TargetContainer::new(system_conf);
        targets.push_all(target_ids)?;
        let collector = GridCollector::new(target_ids.len(), metric_ids);
        Ok(TextOutput {
            targets,
            collector,
            formatters,
        })
    }
}

impl<'a> Output for TextOutput<'a> {
    fn run(&mut self, every_ms: time::Duration, count: Option<u64>) {
        let mut loop_number: u64 = 0;
        let metric_names = self.collector.metric_names();
        let metric_count = metric_names.len();
        let mut table = Table::new();
        for name in metric_names {
            table.push_subtitle(name.to_string());
        }
        loop {
            let with_header = self.targets.refresh(); // must print headers again
            self.targets.collect(&mut self.collector);
            let lines = self.collector.lines();
            if lines.is_empty() {
                eprintln!("no process found")
            } else {
                table.clear_titles();
                table.clear_values();
                for line in lines {
                    let name = match &line.metrics {
                        Some(metrics) => format!("{} [{}]", line.name, metrics.pid,),
                        None => line.name.to_string(),
                    };
                    table.push_title(name);
                    match &line.metrics {
                        Some(metrics) => {
                            for (metric_idx, value) in metrics.series.iter().enumerate() {
                                let fmt = self.formatters.get(metric_idx).unwrap();
                                table.push_value((*fmt)(*value));
                            }
                        }
                        None => {
                            for _ in 0..metric_count {
                                table.push_value("----".to_string());
                            }
                        }
                    }
                }
                table.resize();
                table.print(with_header);
            }

            if let Some(count) = count {
                loop_number += 1;
                if loop_number >= count {
                    break;
                }
            }
            thread::sleep(every_ms);
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_divide() {
        assert_eq!((2, 0), super::divide(8, 4));
        assert_eq!((3, 2), super::divide(11, 3));
    }
}
