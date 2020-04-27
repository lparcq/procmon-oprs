#[cfg(unix)]
extern crate libc;

use std::path::PathBuf;
use structopt::StructOpt;

mod application;
mod collector;
mod format;
mod info;
mod metric;
mod output;
mod targets;

use targets::TargetId;

const APP_NAME: &str = "oprs";

//
// Options
//

const HELP_MESSAGE: &str = "
O(bserve)P(rocess)R(e)s(ourses) displays selected metrics for the system or individual processes.

Without argument, prints the list of available metrics.";

#[derive(StructOpt, Debug)]
#[structopt(name = "oprs", about = HELP_MESSAGE)]
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
}

//
// Main
//

fn start(opt: Opt) -> anyhow::Result<()> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix(APP_NAME)?;

    // Configuration
    let config_file_name = xdg_dirs.place_config_file("settings.toml")?;
    let mut settings = config::Config::default();
    if config_file_name.exists() {
        let config_file = config::File::from(config_file_name);
        settings.merge(config_file)?;
    };
    settings.set("name", APP_NAME)?;
    settings.set("every", opt.every)?;
    if let Some(count) = opt.count {
        settings.set("count", count as i64)?;
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
        eprintln!("no process to monitor, exiting.");
    } else {
        application::run(&settings, &opt.metrics, opt.human_format, &target_ids)?;
    }
    Ok(())
}

fn main() {
    let opt = Opt::from_args();
    loggerv::Logger::new()
        .verbosity(opt.verbose)
        .level(true) // add a tag on the line
        .module_path(false)
        .init()
        .unwrap();

    if opt.metrics.is_empty() {
        dbg!(opt.names);
        application::list_metrics();
    } else if let Err(err) = start(opt) {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}
