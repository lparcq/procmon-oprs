use anyhow::{Context, Result};
use config;
use procfs;
use std::fs::File;
use std::io::Read;
use std::thread;
use std::time;

fn read_pid_file(pid_file: &String) -> Result<i32> {
    let mut file =
        File::open(pid_file).with_context(|| format!("{}: cannot open file", pid_file))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents
        .trim()
        .parse::<i32>()
        .with_context(|| format!("{}: invalid pid file", pid_file))?)
}

fn parse_pid_or_file(pid_or_file: &String) -> Result<i32> {
    let pid = pid_or_file.parse::<i32>().or(read_pid_file(pid_or_file))?;
    Ok(pid)
}

fn print_processes(pids: &Vec<i32>, every: time::Duration, count: Option<u64>) {
    let tps = procfs::ticks_per_second().unwrap();
    println!("{: >5} {: >8} {}", "PID", "TIME", "CMD");
    let mut loop_number = 0;
    loop {
        for pid in pids {
            match procfs::process::Process::new(*pid) {
                Ok(prc) => {
                    let total_time = (prc.stat.utime + prc.stat.stime) as f32 / (tps as f32);
                    println!("{: >5} {: >8} {}", prc.stat.pid, total_time, prc.stat.comm);
                }
                Err(err) => {
                    error!("{:?}", err);
                }
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

pub fn run(settings: &config::Config, processes: &Vec<String>) {
    let mut pids = Vec::with_capacity(processes.len());
    for pid_or_file in processes {
        match parse_pid_or_file(pid_or_file) {
            Ok(pid) => pids.push(pid),
            Err(err) => error!("{:?}", err),
        }
    }
    let every_ms = match settings.get_float("every") {
        Ok(every) => time::Duration::from_millis((every * 1000.0) as u64),
        Err(err) => panic!("{:?}", err),
    };
    let count = settings.get_int("count").map(|c| c as u64).ok();
    print_processes(&pids, every_ms, count);
}
