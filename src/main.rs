extern crate anyhow;
#[macro_use]
extern crate log;
extern crate config;
extern crate console;
extern crate loggerv;
extern crate structopt;
extern crate structopt_derive;
extern crate xdg;

use std::path::PathBuf;

use structopt::StructOpt;

mod application;

const APP_NAME: &str = "procmon";

//
// Options
//

#[derive(StructOpt, Debug)]
#[structopt(name = "procmon", about = "Process monitor.")]
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

    #[structopt(name = "PROCESS", required = true, min_values = 1)]
    process: Vec<String>,
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
    let config_file_name = PathBuf::from(xdg_dirs.place_config_file("settings.toml").unwrap());
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

    application::run(&settings, &opt.process);
}
