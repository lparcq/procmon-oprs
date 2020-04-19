use config;
use procfs::process::Process;
use std::thread;
use std::time;

use crate::targets::{Collector, TargetContainer, TargetError, TargetId};

struct Printer {}

impl Collector for Printer {
    fn error(&mut self, target_number: usize, target_name: &str, err: &TargetError) {
        println!("process #{}: {}: {}", target_number, target_name, err);
    }

    fn collect(&mut self, target_number: usize, target_name: &str, _process: &Process) {
        println!("{: <15} {: >8}", target_name, target_number);
    }
}

fn print_processes(container: &mut TargetContainer, every: time::Duration, count: Option<u64>) {
    //let tps = procfs::ticks_per_second().unwrap();
    let mut loop_number = 0;
    loop {
        let mut printer = Printer {};
        container.collect(&mut printer);
        if let Some(count) = count {
            loop_number += 1;
            if loop_number >= count {
                break;
            }
        }
        thread::sleep(every);
    }
}

pub fn run(settings: &config::Config, target_ids: &Vec<TargetId>) {
    let every_ms = match settings.get_float("every") {
        Ok(every) => time::Duration::from_millis((every * 1000.0) as u64),
        Err(err) => panic!("{:?}", err),
    };
    let count = settings.get_int("count").map(|c| c as u64).ok();
    let mut container = TargetContainer::new();
    container.push_all(target_ids);
    print_processes(&mut container, every_ms, count);
}
