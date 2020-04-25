use anyhow::Result;
use config;
use std::thread;
use std::time;
use thiserror::Error;

use crate::collectors::{Collector, GridCollector, MetricMapper};
use crate::targets::{TargetContainer, TargetId};

#[derive(Error, Debug)]
enum Error {
    #[error("{0}: invalid parameter value")]
    InvalidParameter(&'static str),
}

fn print_processes(
    targets: &mut TargetContainer,
    collector: &mut dyn Collector,
    every: time::Duration,
    count: Option<u64>,
) {
    //let tps = procfs::ticks_per_second().unwrap();
    let mut loop_number = 0;
    let metric_names = collector.metric_names();
    loop {
        targets.collect(collector);
        for line in collector.lines() {
            print!(
                "{} {} {}",
                line.name, line.target_number, line.process_number
            );
            match &line.metrics {
                Some(metrics) => {
                    println!(" {}", metrics.pid);
                    for name in &metric_names {
                        print!("|{:^15}", name);
                    }
                    println!("|");
                    for value in &metrics.series {
                        print!("|{:>15}", value);
                    }
                    println!("|");
                }
                None => println!(": no data"),
            }
        }
        if let Some(count) = count {
            loop_number += 1;
            if loop_number >= count {
                break;
            }
        }
        thread::sleep(every);
    }
}

pub fn list_metrics() {
    let metric_mapper = MetricMapper::new();
    metric_mapper.for_each(|id, name| {
        println!("{:<15}\t{}", name, MetricMapper::help(id));
    })
}

pub fn run(
    settings: &config::Config,
    metric_names: &Vec<String>,
    target_ids: &Vec<TargetId>,
) -> Result<()> {
    let every_ms = time::Duration::from_millis(
        (settings
            .get_float("every")
            .map_err(|_| Error::InvalidParameter("every"))?
            * 1000.0) as u64,
    );
    let metric_mapper = MetricMapper::new();
    let metric_ids = metric_mapper.from_names(metric_names)?;
    let count = settings.get_int("count").map(|c| c as u64).ok();
    let mut targets = TargetContainer::new();
    targets.push_all(target_ids);
    let mut collector = GridCollector::new(target_ids.len(), metric_ids);
    print_processes(&mut targets, &mut collector, every_ms, count);
    Ok(())
}
