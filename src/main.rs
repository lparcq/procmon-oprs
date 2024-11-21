// Oprs -- process monitor for Linux
// Copyright (C) 2020-2024  Laurent Pelecq
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

#![deny(clippy::mem_forget)]

#[cfg(unix)]
extern crate libc;

use argh::FromArgs;
use simplelog::{self, SimpleLogger, TermLogger, WriteLogger};
use std::{
    fs::{self, File},
    panic,
    path::{Path, PathBuf},
    str::FromStr,
};

mod application;
mod cfg;
mod clock;
mod console;
mod display;
mod export;
mod process;
mod sighdr;

use application::Application;
use cfg::{
    BuiltinTheme, DisplayMode, ExportType, LoggingLevel, LoggingSettings, MetricFormat,
    LOG_FILE_NAME,
};
use process::{parsers::parse_size, TargetId};

const APP_NAME: &str = "oprs";

//
// Options
//

macro_rules! make_arg_converter {
    ($func:ident, $type:ty) => {
        fn $func(arg: &str) -> Result<$type, String> {
            <$type>::from_str(arg).map_err(|err| err.to_string())
        }
    };
}

make_arg_converter!(theme_from_str, BuiltinTheme);
make_arg_converter!(export_type_from_str, ExportType);
make_arg_converter!(display_mode_from_str, DisplayMode);
make_arg_converter!(metric_format_from_str, MetricFormat);

#[derive(FromArgs, PartialEq, Debug)]
/// Display metrics of processes.
/// Without argument, the command prints the available metrics.
struct Opt {
    #[argh(switch, short = 'v', description = "verbose mode")]
    verbose: bool,

    #[argh(switch, description = "debug mode")]
    debug: bool,

    #[argh(switch, short = 'l', description = "list the available metrics")]
    list: bool,

    #[argh(option, short = 'L', description = "log file")]
    log_file: Option<String>,

    #[argh(
        option,
        short = 'T',
        from_str_fn(theme_from_str),
        description = "display theme (light, dark, light16, dark16)"
    )]
    theme: Option<BuiltinTheme>,

    #[argh(option, short = 'c', description = "number of loops")]
    count: Option<u64>,

    #[argh(
        option,
        short = 'e',
        description = "delay between two samples (default: 5.0)"
    )]
    every: Option<f64>,

    #[argh(
        option,
        short = 'd',
        from_str_fn(display_mode_from_str),
        description = "display mode, if unset uses terminal in priority (none, any, text, term)"
    )]
    display: Option<DisplayMode>,

    #[argh(
        option,
        short = 'X',
        from_str_fn(export_type_from_str),
        description = "export type (none, csv, rrd)"
    )]
    export_type: Option<ExportType>,

    #[argh(option, short = 'D', description = "export directory")]
    export_dir: Option<String>,

    #[argh(
        option,
        short = 'S',
        description = "export size (for csv, the size of files)."
    )]
    export_size: Option<String>,

    #[argh(
        option,
        short = 'C',
        description = "number of exported items (for csv, the number of files; for rrd, the number of rows)."
    )]
    export_count: Option<usize>,

    #[argh(
        option,
        short = 'U',
        from_str_fn(metric_format_from_str),
        description = "units format to display metrics (raw, human)"
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

fn convert_log_level(log_level: LoggingLevel) -> simplelog::LevelFilter {
    match log_level {
        LoggingLevel::Debug => simplelog::LevelFilter::Debug,
        LoggingLevel::Info => simplelog::LevelFilter::Info,
        LoggingLevel::Warning => simplelog::LevelFilter::Warn,
        LoggingLevel::Error => simplelog::LevelFilter::Error,
    }
}

fn configure_logging(settings: &LoggingSettings) {
    fn configure_console_logging(
        config: simplelog::Config,
        log_level: LoggingLevel,
    ) -> anyhow::Result<()> {
        TermLogger::init(
            convert_log_level(log_level),
            config,
            simplelog::TerminalMode::Mixed,
            simplelog::ColorChoice::Auto,
        )?;
        Ok(())
    }

    fn configure_file_logging(
        config: simplelog::Config,
        log_file: &Path,
        log_level: LoggingLevel,
    ) -> anyhow::Result<()> {
        if log_file.exists() {
            let mut backup_file = log_file.to_path_buf();
            if backup_file.set_extension("log.0") {
                fs::rename(log_file, backup_file)?;
            }
        }
        WriteLogger::init(
            convert_log_level(log_level),
            config,
            File::create(log_file)?,
        )?;
        Ok(())
    }

    let config = simplelog::ConfigBuilder::new()
        .set_time_offset_to_local()
        .unwrap_or_else(|c| c)
        .build();

    match &settings.file {
        Some(ref file) => configure_file_logging(config, file, settings.level),
        None => configure_console_logging(config, settings.level),
    }
    .unwrap_or_else(|_| {
        SimpleLogger::init(
            convert_log_level(settings.level),
            simplelog::Config::default(),
        )
        .expect("cannot initialize logging")
    });
}

//
// Main
//

macro_rules! override_parameter {
    // Assign option to lvalue if option is set.
    ($lvalue:expr, $option:expr) => {
        override_parameter!($lvalue, $option, value, value)
    };
    // Assign rvalue to lvalue if option is set by matching var to the value of option.
    ($lvalue:expr, $option:expr, $var:ident, $($rvalue:tt)*) => {
        if let Some($var) = $option {
            $lvalue = $($rvalue)*;
        }
    };
}

fn start(opt: Opt) -> anyhow::Result<()> {
    // Configuration
    let dirs = cfg::Directories::new(APP_NAME)?;
    let mut settings = dirs.read_config_file(LOG_FILE_NAME)?;

    // Override config file with command line
    override_parameter!(settings.display.mode, opt.display);
    override_parameter!(settings.display.every, opt.every);
    override_parameter!(settings.display.format, opt.format);
    override_parameter!(settings.display.count, opt.count, count, Some(count));
    override_parameter!(settings.display.theme, opt.theme, theme, Some(theme));
    override_parameter!(settings.export.kind, opt.export_type);
    override_parameter!(settings.export.dir, opt.export_dir, dir, PathBuf::from(dir));
    override_parameter!(
        settings.export.size,
        opt.export_size,
        size,
        Some(parse_size(&size)?)
    );
    override_parameter!(settings.export.count, opt.export_count, count, Some(count));

    override_parameter!(
        settings.logging.file,
        opt.log_file,
        file,
        Some(PathBuf::from(file))
    );

    if opt.debug {
        settings.logging.level = LoggingLevel::Debug;
    } else if opt.verbose {
        settings.logging.level = LoggingLevel::Info;
    }

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
    let metric_names = if opt.metric.is_empty() {
        vec!["time:cpu-raw+ratio", "mem:vm", "time:elapsed"]
    } else {
        opt.metric.iter().map(String::as_str).collect::<Vec<&str>>()
    };
    let app = Application::new(&settings, &metric_names)?;
    configure_logging(&settings.logging);
    let must_print_backtrace = opt.debug;

    panic::set_hook(Box::new(move |panic_info| {
        if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            log::error!("panic occurred: {s:?}");
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            log::error!("panic occurred: {s:?}");
        } else if let Some(location) = panic_info.location() {
            log::error!(
                "panic occurred in file '{}' at line {}",
                location.file(),
                location.line(),
            );
        } else {
            log::error!("panic occurred but can't get location information...");
        }
        if must_print_backtrace {
            let bcktrc = std::backtrace::Backtrace::force_capture();
            log::debug!("{bcktrc}");
        }
    }));
    let system_conf = process::SystemConf::new()?;
    if let Err(err) = app.run(&target_ids, &system_conf) {
        log::error!("{}", err);
        if settings.logging.file.is_some() {
            eprintln!("{err}");
        }
    }
    Ok(())
}

fn main() {
    #[cfg(all(debug_assertions, target_os = "linux"))]
    unsafe {
        // Allow debugger to attach
        libc::prctl(libc::PR_SET_PTRACER, -1, 0, 0, 0);
    }

    let opt: Opt = argh::from_env();
    if opt.list {
        application::list_metrics();
    } else if let Err(err) = start(opt) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
