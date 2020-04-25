#[cfg(unix)]
extern crate libc;

use std::path::PathBuf;
use structopt::StructOpt;

mod application;
mod collector;
mod format;
mod metric;
mod output;
mod targets;

use targets::TargetId;

const APP_NAME: &str = "procmon";

//
// Options
//

const HELP_MESSAGE: &str = "
Display selected metrics for the system or individual processes.

Without argument, prints the list of available metrics.";

#[derive(StructOpt, Debug)]
#[structopt(name = "procmon", about = HELP_MESSAGE)]
struct Opt {
    #[structopt(short, long, parse(from_occurrences), help = "Activate verbose mode")]
    verbose: u64,

    #[structopt(short, long, help = "number of loops")]
    count: Option<u64>,

    #[structopt(
        short = "y",
        long,
        help = "delay between two samples",
        default_value = "5"
    )]
    every: f64,

    #[structopt(short = "p", long = "pid", help = "process id")]
    pids: Vec<i32>,

    #[structopt(short = "f", long = "file", help = "process id file")]
    files: Vec<String>,

    #[structopt(short = "n", long = "name", help = "process name")]
    names: Vec<String>,

    #[structopt(name = "METRIC", help = "metric to monitor.")]
    metrics: Vec<String>,
}

//
// Main
//

fn main() {
    let opt = Opt::from_args();
    loggerv::Logger::new()
        .verbosity(opt.verbose)
        .level(true) // add a tag on the line
        .module_path(false)
        .init()
        .unwrap();

    let xdg_dirs = xdg::BaseDirectories::with_prefix(APP_NAME).unwrap();

    // Configuration
    let config_file_name = xdg_dirs.place_config_file("settings.toml").unwrap();
    let mut settings = config::Config::default();
    if config_file_name.exists() {
        let config_file = config::File::from(config_file_name);
        settings.merge(config_file).unwrap();
    };
    settings.set("name", APP_NAME).unwrap();
    settings.set("every", opt.every).unwrap();
    if let Some(count) = opt.count {
        settings.set("count", count as i64).unwrap();
    }

    if opt.pids.is_empty() && opt.files.is_empty() && opt.names.is_empty() {
        application::list_metrics();
    } else {
        let mut target_ids = Vec::new();
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

        if let Err(err) = application::run(&settings, &opt.metrics, &target_ids) {
            eprintln!("{}", err);
            std::process::exit(1);
        };
    }
}
