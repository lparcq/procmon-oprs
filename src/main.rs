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

use argh::FromArgs;
use log::{error, warn};
use simplelog::{self, SimpleLogger, TermLogger, WriteLogger};
use std::collections::HashMap;
use std::fs::{self, File};
use std::path::PathBuf;
use strum_macros::{EnumString, IntoStaticStr};

mod agg;
mod application;
mod cfg;
mod clock;
mod collector;
mod console;
mod display;
mod export;
mod format;
mod info;
mod metrics;
mod proc_dir;
mod sighdr;
mod targets;
mod utils;

#[cfg(test)]
mod mocks;

use application::Application;
use cfg::{DisplayMode, ExportType, MetricFormat};
use targets::TargetId;

const APP_NAME: &str = "oprs";

//
// Options
//

#[derive(Clone, Copy, Debug, PartialEq, EnumString, IntoStaticStr)]
enum LoggingTarget {
    #[strum(serialize = "console")]
    Console,
    #[strum(serialize = "file")]
    File,
}

#[derive(Clone, Copy, Debug, PartialEq, EnumString)]
enum ColorTheme {
    #[strum(serialize = "none")]
    None,
    #[strum(serialize = "light")]
    Light,
    #[strum(serialize = "dark")]
    Dark,
}

const DEFAULT_DELAY: f64 = 5.0;

#[derive(FromArgs, PartialEq, Debug)]
/// Display procfs metrics of processes
struct Opt {
    #[argh(switch, short = 'v', description = "verbose mode")]
    verbose: bool,

    #[argh(switch, description = "debug mode")]
    debug: bool,

    #[argh(
        option,
        short = 'L',
        from_str_fn(LoggingTarget::from_str),
        description = "logging target"
    )]
    logging: Option<LoggingTarget>,

    #[argh(
        option,
        short = 'T',
        from_str_fn(ColorTheme::from_str),
        description = "color theme"
    )]
    color_theme: Option<ColorTheme>,

    #[argh(option, short = 'c', description = "number of loops")]
    count: Option<u64>,

    #[argh(
        option,
        short = 'y',
        description = "delay between two samples (default: 5.0)"
    )]
    every: Option<f64>,

    #[argh(
        option,
        short = 'd',
        from_str_fn(DisplayMode::from_str),
        description = "display mode, if unset uses terminal in priority"
    )]
    display: Option<DisplayMode>,

    #[argh(
        option,
        short = 'X',
        from_str_fn(ExportType::from_str),
        description = "export type"
    )]
    export_type: Option<ExportType>,

    #[argh(option, short = 'D', description = "export directory")]
    export_dir: Option<String>,

    #[argh(
        option,
        short = 'S',
        description = "export size (for rrd, the number of rows)."
    )]
    export_size: Option<usize>,

    #[argh(
        option,
        short = 'F',
        from_str_fn(MetricFormat::from_str),
        description = "format to display metrics"
    )]
    format: Option<MetricFormat>,

    #[argh(switch, short = 's', description = "monitor system")]
    system: bool,

    #[argh(switch, description = "monitor the command itself")]
    myself: bool,

    #[argh(option, short = 'p', description = "process id")]
    pid: Vec<i32>,

    #[argh(option, short = 'f', description = "process id file")]
    file: Vec<String>,

    #[argh(option, short = 'n', description = "process name")]
    name: Vec<String>,

    #[argh(positional, description = "metric to monitor")]
    metric: Vec<String>,
}

//
// Logging
//

fn configure_logging(dirs: &cfg::Directories, verbose: bool, debug: bool, target: LoggingTarget) {
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
    let log_level = if debug {
        simplelog::LevelFilter::Debug
    } else if verbose {
        simplelog::LevelFilter::Info
    } else {
        simplelog::LevelFilter::Warn
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

/// Set an export parameter from command line if found
macro_rules! override_export_parameter {
    // If the parameter is mandatory, the last argument is an error message.
    ($key_name:ident, $value:expr, $params:ident, $defaults:expr, $errmsg:expr) => {
        $params.insert(
            String::from(cfg::$key_name),
            match &$value {
                Some(ref value) => value.as_str().to_string(),
                None => $defaults
                    .get(cfg::$key_name)
                    .expect($errmsg)
                    .clone()
                    .into_str()?,
            },
        );
    };
    // If the parameter is optional, it can be omitted.
    ($key_name:ident, $value:expr, $params:ident, $defaults:expr) => {
        match &$value {
            Some(ref value) => {
                $params.insert(String::from(cfg::$key_name), value.to_string());
            }
            None => {
                if let Some(value) = $defaults.get(cfg::$key_name) {
                    $params.insert(String::from(cfg::$key_name), value.to_string());
                }
            }
        };
    };
}

/// Wrapper for anyhow to convert String to anyhow::Error
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    String(String),
    #[error("{0}")]
    Config(config::ConfigError),
}

fn read_config(mut settings: &mut config::Config, dirs: &cfg::Directories) -> anyhow::Result<()> {
    settings.set_default(cfg::KEY_EVERY, DEFAULT_DELAY)?;
    settings.set_default(cfg::KEY_METRIC_FORMAT, String::from("raw"))?;
    let mut export_settings = HashMap::new();
    export_settings.insert(String::from(cfg::KEY_EXPORT_DIR), String::from("."));
    export_settings.insert(String::from(cfg::KEY_EXPORT_TYPE), String::from("none"));
    settings.set_default(cfg::KEY_EXPORT, config::Value::from(export_settings))?;
    if let Ok(config_reader) = cfg::Reader::new(&dirs) {
        config_reader.read_config_file(&mut settings, "settings")?;
    }
    settings.set(cfg::KEY_APP_NAME, APP_NAME)?;
    Ok(())
}

fn merge_export_parameters(
    settings: &config::Config,
    options: &Opt,
) -> anyhow::Result<HashMap<String, String>> {
    let default_export_settings = settings.get_table(cfg::KEY_EXPORT)?;
    let mut export_settings = HashMap::new();
    override_export_parameter!(
        KEY_EXPORT_TYPE,
        options.export_type,
        export_settings,
        default_export_settings,
        "internal error: export type not set as default"
    );
    override_export_parameter!(
        KEY_EXPORT_DIR,
        options.export_dir,
        export_settings,
        default_export_settings,
        "internal error: export directory not set as default"
    );
    override_export_parameter!(
        KEY_EXPORT_SIZE,
        options.export_size.map(|val| format!("{}", val)),
        export_settings,
        default_export_settings
    );
    Ok(export_settings)
}

fn start(dirs: &cfg::Directories, opt: Opt) -> anyhow::Result<()> {
    // Configuration
    let mut settings = config::Config::default();
    read_config(&mut settings, dirs)?;
    // Override config file with command line
    if let Some(every) = opt.every {
        settings.set(cfg::KEY_EVERY, every)?;
    }
    if let Some(count) = opt.count {
        settings.set(cfg::KEY_COUNT, count as i64)?;
    }
    if let Some(theme) = opt.color_theme {
        settings.set(
            cfg::KEY_COLOR_THEME,
            match theme {
                ColorTheme::Light => "light",
                ColorTheme::Dark => "dark",
                _ => "none",
            },
        )?;
    };
    if let Some(format) = opt.format {
        settings.set(cfg::KEY_METRIC_FORMAT, format.as_str())?;
    }
    if let Some(display_mode) = opt.display {
        settings.set(cfg::KEY_DISPLAY_MODE, display_mode.as_str())?;
    }

    let export_settings = merge_export_parameters(&settings, &opt)?;
    settings.set(cfg::KEY_EXPORT, config::Value::from(export_settings))?;

    // Add targets
    let mut target_ids = Vec::new();
    if opt.system {
        target_ids.push(TargetId::System);
    }
    if opt.myself {
        target_ids.push(TargetId::Pid(std::process::id() as libc::pid_t));
    }
    for pid in opt.pid {
        target_ids.push(TargetId::Pid(pid));
    }
    for pid_file in opt.file {
        let path = PathBuf::from(pid_file.as_str());
        target_ids.push(TargetId::PidFile(path));
    }
    for name in opt.name {
        target_ids.push(TargetId::ProcessName(name));
    }
    if target_ids.is_empty() {
        warn!("no process to monitor, exiting.");
    } else {
        let mut app = Application::new(&settings, &opt.metric)?;
        configure_logging(
            &dirs,
            opt.verbose,
            opt.debug,
            opt.logging.unwrap_or(LoggingTarget::Console),
        );
        let system_conf = info::SystemConf::new()?;
        if let Err(err) = app.run(&target_ids, &system_conf) {
            error!("{}", err);
        }
    }
    Ok(())
}

fn main() {
    if let Ok(dirs) = cfg::Directories::new(APP_NAME) {
        let opt: Opt = argh::from_env();
        if opt.metric.is_empty() {
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
