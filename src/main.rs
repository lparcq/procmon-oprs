// Oprs -- process monitor for Linux
// Copyright (C) 2020  Laurent Pelecq
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

#[cfg(unix)]
extern crate libc;

use clap::arg_enum;
use log::{error, warn};
use simplelog::{self, SimpleLogger, TermLogger, WriteLogger};
use std::fs::{self, File};
use std::path::PathBuf;
use structopt::StructOpt;

mod agg;
mod application;
mod cfg;
mod collector;
mod format;
mod info;
mod metrics;
mod output;
mod targets;
mod utils;

#[cfg(test)]
mod mocks;

use application::{Application, OutputType};
use targets::TargetId;

const APP_NAME: &str = "oprs";

//
// Options
//

const HELP_MESSAGE: &str = "
O(bserve)P(rocess)R(e)s(ourses) displays selected metrics for the system or individual processes.

Without argument, prints the list of available metrics.

Limited patterns are allowed for metrics: by prefix mem:*, suffix *:call, both io:*:count.

A metric may be followed by a unit. For example: mem:vm/gi

Available units:
ki  kibi
mi  mebi
gi  gibi
ti  tebi
k   kilo
m   mega
g   giga
t   tera
sz  the best unit in k, m, g or t.
du  format duration as hour, minutes, seconds.

Example:
  oprs --system -n bash -p 1234 -m mem:vm time:real
";

arg_enum! {
    #[derive(Clone, Copy, Debug)]
    enum LoggingTarget {
        Console,
        File,
    }
}

#[derive(StructOpt, Debug)]
#[structopt(name = APP_NAME, about = HELP_MESSAGE)]
struct Opt {
    #[structopt(short, long, parse(from_occurrences), help = "activate verbose mode")]
    verbose: u8,

    #[structopt(
        short = "L",
        long = "logging",
        help = "logging target (console, file)",
        default_value = "console"
    )]
    logging_target: LoggingTarget,

    #[structopt(short, long, help = "number of loops")]
    count: Option<u64>,

    #[structopt(
        short = "y",
        long,
        help = "delay between two samples",
        default_value = "5"
    )]
    every: f64,

    #[structopt(short = "H", long = "human", help = "use human-readable units")]
    human_format: bool,

    #[structopt(short, long, help = "monitor system")]
    system: bool,

    #[structopt(short = "S", long = "self", help = "monitor the command itself")]
    myself: bool,

    #[structopt(short = "p", long = "pid", help = "process id")]
    pids: Vec<i32>,

    #[structopt(short = "f", long = "file", help = "process id file")]
    files: Vec<String>,

    #[structopt(short = "n", long = "name", help = "process name")]
    names: Vec<String>,

    #[structopt(short = "m", long = "metric", help = "metric to monitor.")]
    metrics: Vec<String>,

    #[structopt(short, long, possible_values = &OutputType::variants(), case_insensitive = true, default_value = "any")]
    output: OutputType,
}

//
// Logging
//

fn configure_logging(dirs: &cfg::Directories, verbosity: u8, target: LoggingTarget) {
    fn configure_console_logging(log_level: simplelog::LevelFilter) -> anyhow::Result<()> {
        TermLogger::init(
            log_level,
            simplelog::Config::default(),
            simplelog::TerminalMode::Mixed,
        )?;
        Ok(())
    }
    fn configure_file_logging(
        dirs: &cfg::Directories,
        log_level: simplelog::LevelFilter,
    ) -> anyhow::Result<()> {
        let log_path = dirs.get_log_file()?;
        if log_path.exists() {
            let mut backup_path = log_path.clone();
            if backup_path.set_extension("log.0") {
                fs::rename(log_path.as_path(), backup_path)?;
            }
        }
        WriteLogger::init(
            log_level,
            simplelog::Config::default(),
            File::create(log_path)?,
        )?;
        Ok(())
    }
    let log_level = match verbosity {
        0 => simplelog::LevelFilter::Off,
        1 => simplelog::LevelFilter::Error,
        2 => simplelog::LevelFilter::Warn,
        3 => simplelog::LevelFilter::Info,
        4 => simplelog::LevelFilter::Debug,
        _ => simplelog::LevelFilter::Trace,
    };
    match target {
        LoggingTarget::Console => configure_console_logging(log_level),
        LoggingTarget::File => configure_file_logging(&dirs, log_level),
    }
    .unwrap_or_else(|_| {
        SimpleLogger::init(log_level, simplelog::Config::default())
            .expect("cannot initialize logging")
    });
}

//
// Main
//

fn start(dirs: &cfg::Directories, opt: Opt) -> anyhow::Result<()> {
    // Configuration
    let mut settings = config::Config::default();
    if let Ok(config_reader) = cfg::Reader::new(&dirs) {
        config_reader.read_config_file(&mut settings, "settings")?;
    }
    settings.set(cfg::KEY_APP_NAME, APP_NAME)?;
    settings.set(cfg::KEY_EVERY, opt.every)?;
    cfg::provide(&mut settings, cfg::KEY_HUMAN_FORMAT, opt.human_format)?;
    if let Some(count) = opt.count {
        settings.set(cfg::KEY_COUNT, count as i64)?;
    }

    let mut target_ids = Vec::new();
    if opt.system {
        target_ids.push(TargetId::System);
    }
    if opt.myself {
        target_ids.push(TargetId::Pid(std::process::id() as libc::pid_t));
    }
    for pid in opt.pids {
        target_ids.push(TargetId::Pid(pid));
    }
    for pid_file in opt.files {
        let path = PathBuf::from(pid_file.as_str());
        target_ids.push(TargetId::PidFile(path));
    }
    for name in opt.names {
        target_ids.push(TargetId::ProcessName(name));
    }
    if target_ids.is_empty() {
        warn!("no process to monitor, exiting.");
    } else {
        let mut app = Application::new(&settings, &opt.metrics)?;
        configure_logging(&dirs, opt.verbose + 1, opt.logging_target);
        let system_conf = info::SystemConf::new()?;
        if let Err(err) = app.run(opt.output, &target_ids, &system_conf) {
            error!("{}", err);
        }
    }
    Ok(())
}

fn main() {
    if let Ok(dirs) = cfg::Directories::new(APP_NAME) {
        let opt = Opt::from_args();
        if opt.metrics.is_empty() {
            application::list_metrics();
        } else if let Err(err) = start(&dirs, opt) {
            eprintln!("{}", err);
            std::process::exit(1);
        }
    } else {
        eprintln!("cannot initialize directories");
        std::process::exit(1);
    }
}
