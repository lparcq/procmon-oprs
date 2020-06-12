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

use clap::arg_enum;
use config::ConfigError;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use thiserror::Error;

pub const KEY_APP_NAME: &str = "name";
pub const KEY_COLOR_THEME: &str = "theme";
pub const KEY_COUNT: &str = "count";
pub const KEY_DISPLAY_MODE: &str = "display";
pub const KEY_EVERY: &str = "every";
pub const KEY_EXPORT: &str = "export";
pub const KEY_EXPORT_DIR: &str = "dir";
pub const KEY_EXPORT_TYPE: &str = "type";
pub const KEY_METRIC_FORMAT: &str = "format";

arg_enum! {
    #[derive(Debug)]
    pub enum DisplayMode {
        None,
        Any,
        Text,
        Term,
    }
}

arg_enum! {
    #[derive(Debug)]
    pub enum ExportType {
        None,
        Csv
    }
}

arg_enum! {
    #[derive(Clone, Copy, Debug)]
    pub enum MetricFormat {
        Raw,
        Human,
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}: invalid configuration entry")]
    InvalidConfigurationEntry(&'static str),
    #[error("{0}: invalid parameter value")]
    InvalidParameter(&'static str),
    #[error("{0}: unknown display mode")]
    UnknownDisplayMode(String),
    #[error("{0}: unknown export type")]
    UnknownExportType(String),
    #[error("{0}: unknown metric format")]
    UnknownMetricFormat(String),
}

const EXTENSIONS: &[&str] = &["toml", "yaml", "json"];

pub struct Directories {
    app_name: String,
    xdg_dirs: xdg::BaseDirectories,
}

impl Directories {
    pub fn new(app_name: &str) -> anyhow::Result<Directories> {
        Ok(Directories {
            app_name: String::from(app_name),
            xdg_dirs: xdg::BaseDirectories::with_prefix(app_name)?,
        })
    }

    /// Path of the log file in the runtime directory
    pub fn get_log_file(&self) -> anyhow::Result<PathBuf> {
        let basename = format!("{}.log", self.app_name);
        let path = xdg::BaseDirectories::new()?.place_runtime_file(basename)?;
        Ok(path)
    }

    fn config_file_in_dir<P>(name: &str, dir: P) -> Option<PathBuf>
    where
        P: AsRef<Path>,
    {
        for extension in EXTENSIONS {
            let basename = format!("{}.{}", name, extension);
            let path = dir.as_ref().join(basename);
            if path.exists() {
                return Some(path);
            }
        }
        None
    }

    /// Return the first config file in the path with extension .toml, .yaml, .json
    pub fn first_config_file(&self, name: &str) -> Option<PathBuf> {
        let home = self.xdg_dirs.get_config_home();
        Directories::config_file_in_dir(name, home).or_else(|| {
            for dir in self.xdg_dirs.get_config_dirs() {
                if let Some(path) = Directories::config_file_in_dir(name, dir) {
                    return Some(path);
                }
            }
            None
        })
    }
}

pub struct Reader<'a> {
    dirs: &'a Directories,
}

impl<'a> Reader<'a> {
    pub fn new(dirs: &'a Directories) -> anyhow::Result<Reader> {
        Ok(Reader { dirs })
    }

    /// Read config file searching for extension .toml, .yaml, .json.
    pub fn read_config_file(&self, config: &mut config::Config, name: &str) -> anyhow::Result<()> {
        if let Some(config_file_name) = self.dirs.first_config_file(name) {
            let config_file = config::File::from(config_file_name);
            config.merge(config_file)?;
        }
        Ok(())
    }
}

/// Return the delay for collecting metrics
pub fn get_every(settings: &config::Config) -> anyhow::Result<Duration> {
    Ok(Duration::from_millis(
        (settings
            .get_float(KEY_EVERY)
            .map_err(|_| Error::InvalidParameter(KEY_EVERY))?
            * 1000.0) as u64,
    ))
}

/// Return the best display mode for any and check if mode is available otherwise.
pub fn get_display_mode(settings: &config::Config) -> anyhow::Result<DisplayMode> {
    match settings.get_str(KEY_DISPLAY_MODE) {
        Ok(value) => {
            let name = value.as_str();
            Ok(DisplayMode::from_str(name)
                .map_err(|_| Error::UnknownDisplayMode(name.to_string()))?)
        }
        Err(ConfigError::NotFound(_)) => Ok(DisplayMode::Any),
        _ => Err(Error::InvalidConfigurationEntry(KEY_DISPLAY_MODE))?,
    }
}

/// Get metric format
pub fn get_metric_format(settings: &config::Config) -> anyhow::Result<MetricFormat> {
    let name = settings.get_str(KEY_METRIC_FORMAT)?;
    let format =
        MetricFormat::from_str(&name).map_err(|_| Error::UnknownMetricFormat(name.to_string()))?;
    Ok(format)
}

/// Get export parameter: type and directory
pub fn get_export_parameters(settings: &config::Config) -> anyhow::Result<(ExportType, PathBuf)> {
    match settings.get_table(KEY_EXPORT) {
        Ok(settings) => {
            let export_type = match settings.get(KEY_EXPORT_TYPE) {
                Some(value) => {
                    let name = value.clone().into_str()?;
                    ExportType::from_str(&name)
                        .map_err(|_| Error::UnknownExportType(name.to_string()))?
                }
                None => ExportType::None,
            };
            let export_dir = PathBuf::from(match settings.get(KEY_EXPORT_DIR) {
                Some(value) => value.clone().into_str()?,
                None => String::from("."),
            });
            Ok((export_type, export_dir))
        }
        Err(ConfigError::NotFound(_)) => Ok((ExportType::None, PathBuf::from("."))),
        _ => Err(Error::InvalidConfigurationEntry(KEY_EXPORT))?,
    }
}
