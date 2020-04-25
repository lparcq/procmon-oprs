use std::thread;
use std::time;

use super::Output;
use crate::collectors::{Collector, GridCollector, MetricId};
use crate::targets::{TargetContainer, TargetId};

/// Roughly calculate number width in base 10 without logarithm
fn number_width(number: u64) -> usize {
    if number < 1000 {
        3
    } else if number < 10_000 {
        4
    } else if number < 1_000_000 {
        6
    } else if number < 100_000_000 {
        8
    } else if number < 10_000_000_000 {
        10
    } else {
        20
    }
}

fn divide(numerator: usize, denominator: usize) -> (usize, usize) {
    let quotient = numerator / denominator;
    (quotient, numerator - quotient * denominator)
}

pub struct TextOutput {
    targets: TargetContainer,
    collector: GridCollector,
}

impl TextOutput {
    pub fn new(target_ids: &[TargetId], metric_ids: Vec<MetricId>) -> TextOutput {
        let mut targets = TargetContainer::new();
        targets.push_all(target_ids);
        let collector = GridCollector::new(target_ids.len(), metric_ids);
        TextOutput { targets, collector }
    }
}

impl Output for TextOutput {
    fn run(&mut self, every_ms: time::Duration, count: Option<u64>) {
        let repeat_header_every = 20;
        let mut loop_number: u64 = 0;
        let metric_names = self.collector.metric_names();
        let metric_count = metric_names.len();
        let mut col_width = 0;
        let mut repeat: u16 = 0;
        loop {
            self.targets.collect(&mut self.collector);
            let lines = self.collector.lines();
            let line_count = lines.len();
            // Calculate the column width
            for line in lines {
                // minimum column with to display the process name
                let (quotient, remainder) = divide(line.name.len() + 8 + 3, metric_count);
                let min_col_width = quotient - 3 + if remainder > 0 { 1 } else { 0 };
                if min_col_width > col_width {
                    col_width = min_col_width;
                    repeat = 0; // must print headers again
                }
                if let Some(metrics) = &line.metrics {
                    for value in &metrics.series {
                        let width = number_width(*value);
                        if width > col_width {
                            col_width = width;
                            repeat = 0; // must print headers again
                        }
                    }
                }
            }
            //let name_width = (col_width + 2) * metric_count - 1;
            let name_width = (col_width + 3) * metric_count - 3;
            // Print headers from time to time
            if repeat == 0 {
                // An horizontal rule
                for _ in 0..line_count {
                    print!("|{:-<width$}", "", width = name_width + 2);
                }
                println!("|");
                // The process names
                for line in lines {
                    let name = format!(
                        "{} [{}]",
                        line.name,
                        match &line.metrics {
                            Some(metrics) => metrics.pid,
                            None => -1,
                        }
                    );
                    print!("| {:^width$} ", name, width = name_width);
                }
                println!("|");
                // The metric names
                for _ in 0..line_count {
                    for name in &metric_names {
                        print!("| {:^width$} ", name, width = col_width);
                    }
                }
                println!("|");
            }
            // Print values
            for line in lines {
                match &line.metrics {
                    Some(metrics) => {
                        for value in &metrics.series {
                            print!("| {:^width$} ", value, width = col_width);
                        }
                    }
                    None => {
                        for _ in 0..metric_count {
                            print!("| {:^width$} ", "----", width = col_width);
                        }
                    }
                }
            }
            println!("|");
            repeat += 1;
            if repeat >= repeat_header_every {
                repeat = 0;
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
    fn test_number_width() {
        assert_eq!(3, super::number_width(0));
        assert_eq!(3, super::number_width(999));
        assert_eq!(4, super::number_width(1_000));
        assert_eq!(4, super::number_width(9_999));
        assert_eq!(6, super::number_width(10_000));
        assert_eq!(6, super::number_width(999_999));
        assert_eq!(8, super::number_width(1_000_000));
        assert_eq!(8, super::number_width(99_999_999));
        assert_eq!(10, super::number_width(100_000_000));
        assert_eq!(10, super::number_width(9_999_999_999));
        assert_eq!(20, super::number_width(10_000_000_000));
        assert_eq!(20, super::number_width(std::u64::MAX));
    }

    #[test]
    fn test_divide() {
        assert_eq!((2, 0), super::divide(8, 4));
        assert_eq!((3, 2), super::divide(11, 3));
    }
}
