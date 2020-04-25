use std::thread;
use std::time;

use super::Output;
use crate::collectors::{Collector, GridCollector, MetricId};
use crate::targets::{TargetContainer, TargetId};

pub struct TextOutput {
    targets: TargetContainer,
    collector: GridCollector,
}

impl TextOutput {
    pub fn new(target_ids: &Vec<TargetId>, metric_ids: Vec<MetricId>) -> TextOutput {
        let mut targets = TargetContainer::new();
        targets.push_all(target_ids);
        let collector = GridCollector::new(target_ids.len(), metric_ids);
        TextOutput { targets, collector }
    }
}

impl Output for TextOutput {
    fn run(&mut self, every_ms: time::Duration, count: Option<u64>) {
        let repeat_header_every = 20;
        let mut loop_number = 0;
        let metric_names = self.collector.metric_names();
        let col_width = 15;
        let name_width = (col_width + 2) * metric_names.len() - 1;
        loop {
            self.targets.collect(&mut self.collector);
            let lines = self.collector.lines();
            let line_count = lines.len();
            if loop_number % repeat_header_every == 0 {
                let mut sep = "|";
                for _ in 0..line_count {
                    print!("{}{:-<width$}", sep, "", width = name_width + 2);
                    sep = "+";
                }
                println!("|");
                for line in lines {
                    let name = format!(
                        "{} [{}]",
                        line.name,
                        match &line.metrics {
                            Some(metrics) => metrics.pid,
                            None => -1,
                        }
                    );
                    print!("| {:width$} ", name, width = name_width);
                }
                println!("|");
                for _ in 0..line_count {
                    for name in &metric_names {
                        print!("| {:^width$} ", name, width = col_width);
                    }
                }
                println!("|");
            }
            for line in lines {
                match &line.metrics {
                    Some(metrics) => {
                        for value in &metrics.series {
                            print!("| {:^width$} ", value, width = col_width);
                        }
                    }
                    None => {
                        for _ in 0..metric_names.len() {
                            print!("| {:^width$} ", "----", width = col_width);
                        }
                    }
                }
            }
            println!("|");
            loop_number += 1;
            if let Some(count) = count {
                if loop_number >= count {
                    break;
                }
            }
            thread::sleep(every_ms);
        }
    }
}
