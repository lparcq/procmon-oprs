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

use light_ini::{IniHandler, IniParser};
use std::{path::PathBuf, str::FromStr};
use strum_macros::{EnumString, IntoStaticStr};

use crate::process::parsers::parse_size;

pub use crate::console::BuiltinTheme;

pub const DEFAULT_DELAY: f64 = 5.0;
pub const LOG_FILE_NAME: &str = "settings";

#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumString, IntoStaticStr)]
pub enum LoggingLevel {
    #[strum(serialize = "error")]
    Error,
    #[strum(serialize = "warning")]
    Warning,
    #[strum(serialize = "info")]
    Info,
    #[strum(serialize = "debug")]
    Debug,
}

#[derive(Clone, Copy, Debug, EnumString, IntoStaticStr, PartialEq, Eq)]
pub enum DisplayMode {
    #[strum(serialize = "none")]
    None,
    #[strum(serialize = "any")]
    Any,
    #[strum(serialize = "text")]
    Text,
    #[strum(serialize = "term")]
    Terminal,
}

impl DisplayMode {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

#[derive(Clone, Copy, Debug, EnumString, IntoStaticStr, PartialEq, Eq)]
pub enum ExportType {
    #[strum(serialize = "none")]
    None,
    #[strum(serialize = "csv")]
    Csv,
    #[strum(serialize = "tsv")]
    Tsv,
    #[strum(serialize = "rrd")]
    Rrd,
    #[strum(serialize = "rrd-graph")]
    RrdGraph,
}

impl ExportType {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

#[derive(Clone, Copy, Debug, EnumString, IntoStaticStr, PartialEq, Eq)]
pub enum MetricFormat {
    #[strum(serialize = "raw")]
    Raw,
    #[strum(serialize = "human")]
    Human,
}

impl MetricFormat {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("{0}: invalid section")]
    InvalidSection(String),
    #[error("{0}: invalid parameter name")]
    InvalidOption(String),
    #[error("{0}: invalid parameter value")]
    InvalidParameter(String),
    #[error("{0}: unknown export type")]
    UnknownExportType(String),
}

/// Parameters for display
#[derive(Debug)]
pub struct DisplaySettings {
    pub mode: DisplayMode,
    pub every: f64,
    pub count: Option<u64>,
    pub format: MetricFormat,
    pub theme: Option<BuiltinTheme>,
}

impl DisplaySettings {
    fn new() -> DisplaySettings {
        DisplaySettings {
            mode: DisplayMode::Any,
            every: DEFAULT_DELAY,
            count: None,
            format: MetricFormat::Human,
            theme: None,
        }
    }
}

/// Parameters for export
#[derive(Debug)]
pub struct ExportSettings {
    pub kind: ExportType,
    pub dir: PathBuf,
    pub size: Option<u64>,
    pub count: Option<usize>,
}

impl ExportSettings {
    fn new() -> ExportSettings {
        ExportSettings {
            kind: ExportType::None,
            dir: PathBuf::from("."),
            size: None,
            count: None,
        }
    }
}

/// Parameters for logging
pub struct LoggingSettings {
    pub file: Option<PathBuf>,
    pub level: LoggingLevel,
}

impl LoggingSettings {
    fn new() -> LoggingSettings {
        LoggingSettings {
            file: None,
            level: LoggingLevel::Warning,
        }
    }
}

/// Parameters for special targets
pub struct TargetSettings {
    pub system: bool,
    pub myself: bool,
}

impl TargetSettings {
    fn new() -> TargetSettings {
        TargetSettings {
            system: false,
            myself: false,
        }
    }
}

/// Parameters for the application
pub struct Settings {
    pub display: DisplaySettings,
    pub export: ExportSettings,
    pub logging: LoggingSettings,
    pub targets: TargetSettings,
}

impl Settings {
    fn new() -> Settings {
        Settings {
            display: DisplaySettings::new(),
            export: ExportSettings::new(),
            logging: LoggingSettings::new(),
            targets: TargetSettings::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, EnumString)]
enum ConfigSection {
    #[strum(serialize = "display")]
    Display,
    #[strum(serialize = "export")]
    Export,
    #[strum(serialize = "logging")]
    Logging,
    #[strum(serialize = "targets")]
    Targets,
}

/// Configuration handler
struct ConfigHandler<'a> {
    section: Option<ConfigSection>,
    settings: &'a mut Settings,
}

impl<'a> ConfigHandler<'a> {
    fn new(settings: &'a mut Settings) -> ConfigHandler<'a> {
        ConfigHandler {
            section: None,
            settings,
        }
    }

    fn parse_bool(key: &str, value: &str) -> Result<bool, ConfigError> {
        match value {
            "yes" | "true" => Ok(true),
            "no" | "false" => Ok(false),
            _ => Err(ConfigError::InvalidParameter(key.to_string())),
        }
    }
}

macro_rules! from_param {
    ($key:expr, $res:expr) => {
        $res.map_err(|_| ConfigError::InvalidParameter($key.to_string()))
    };
    ($enum:ident, $key:expr, $value:expr) => {
        from_param!($key, $enum::from_str($value))
    };
}

impl<'a> IniHandler for ConfigHandler<'a> {
    type Error = ConfigError;

    fn section(&mut self, name: &str) -> Result<(), Self::Error> {
        self.section = Some(
            ConfigSection::from_str(name)
                .map_err(|_| ConfigError::InvalidSection(name.to_string()))?,
        );
        Ok(())
    }

    fn option(&mut self, key: &str, value: &str) -> Result<(), Self::Error> {
        match &self.section {
            None => return Err(ConfigError::InvalidOption(key.to_string())),
            Some(ConfigSection::Display) => {
                let settings = &mut self.settings.display;
                match key {
                    "mode" => settings.mode = from_param!(DisplayMode, key, value)?,
                    "every" => settings.every = from_param!(key, value.parse::<f64>())?,
                    "format" => settings.format = from_param!(MetricFormat, key, value)?,
                    "theme" => settings.theme = Some(from_param!(BuiltinTheme, key, value)?),
                    _ => return Err(ConfigError::InvalidOption(key.to_string())),
                }
            }
            Some(ConfigSection::Export) => {
                let settings = &mut self.settings.export;
                match key {
                    "kind" => {
                        settings.kind = ExportType::from_str(value)
                            .map_err(|_| ConfigError::UnknownExportType(value.to_string()))?
                    }
                    "dir" | "directory" => settings.dir = PathBuf::from(value),
                    "size" => settings.size = Some(from_param!(key, parse_size(value))?),
                    "count" => settings.count = Some(from_param!(key, value.parse::<usize>())?),
                    _ => return Err(ConfigError::InvalidOption(key.to_string())),
                }
            }
            Some(ConfigSection::Logging) => {
                let settings = &mut self.settings.logging;
                match key {
                    "file" => settings.file = Some(PathBuf::from(value)),
                    "level" => settings.level = from_param!(LoggingLevel, key, value)?,
                    _ => return Err(ConfigError::InvalidOption(key.to_string())),
                }
            }
            Some(ConfigSection::Targets) => {
                let settings = &mut self.settings.targets;
                match key {
                    "system" => settings.system = ConfigHandler::parse_bool(key, value)?,
                    "myself" => settings.myself = ConfigHandler::parse_bool(key, value)?,
                    _ => return Err(ConfigError::InvalidOption(key.to_string())),
                }
            }
        }
        Ok(())
    }
}

/// Access to standard directories
pub struct Directories {
    xdg_dirs: xdg::BaseDirectories,
}

impl Directories {
    pub fn new(app_name: &str) -> anyhow::Result<Directories> {
        Ok(Directories {
            xdg_dirs: xdg::BaseDirectories::with_prefix(app_name)?,
        })
    }

    /// Return the first config file in the path
    fn first_config_file(&self, name: &str) -> Option<PathBuf> {
        let basename = format!("{name}.ini");
        self.xdg_dirs.find_config_file(basename)
    }

    /// Read INI configuration file
    pub fn read_config_file(&self, name: &str) -> anyhow::Result<Settings> {
        let mut settings = Settings::new();
        if let Some(config_file_name) = self.first_config_file(name) {
            let mut handler = ConfigHandler::new(&mut settings);
            let mut parser = IniParser::new(&mut handler);
            parser.parse_file(config_file_name)?;
        }
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {

    use std::io::{self, Seek, Write};
    use std::path::PathBuf;

    use super::{
        BuiltinTheme, ConfigHandler, DisplayMode, ExportType, IniParser, LoggingLevel,
        MetricFormat, Settings,
    };

    const VALID_INI: &str = "[display]
mode = term
every = 10
format = human
theme = light

[export]
kind = rrd
dir = /tmp
size = 10m
count = 5

[logging]
file = /var/log/oprs.log
level = info

[targets]
system = true
myself = yes
";

    #[test]
    fn parse_valid_ini() -> io::Result<()> {
        let mut buf = io::Cursor::new(Vec::<u8>::new());
        write!(buf, "{VALID_INI}")?;
        buf.rewind()?;
        let mut settings = Settings::new();
        assert_eq!(DisplayMode::Any, settings.display.mode);
        assert_eq!(super::DEFAULT_DELAY, settings.display.every);
        assert_eq!(MetricFormat::Human, settings.display.format);
        assert_eq!(None, settings.display.theme);
        assert_eq!(ExportType::None, settings.export.kind);
        assert_eq!(PathBuf::from("."), settings.export.dir);
        assert_eq!(None, settings.export.size);
        assert_eq!(None, settings.logging.file);
        assert_eq!(LoggingLevel::Warning, settings.logging.level);
        assert!(!settings.targets.system);
        assert!(!settings.targets.myself);

        let mut handler = ConfigHandler::new(&mut settings);
        let mut parser = IniParser::new(&mut handler);
        parser.parse(buf).unwrap();

        assert_eq!(DisplayMode::Terminal, settings.display.mode);
        assert_eq!(10.0, settings.display.every);
        assert_eq!(MetricFormat::Human, settings.display.format);
        assert_eq!(Some(BuiltinTheme::Light), settings.display.theme);
        assert_eq!(ExportType::Rrd, settings.export.kind);
        assert_eq!(PathBuf::from("/tmp"), settings.export.dir);
        assert_eq!(Some(10_000_000), settings.export.size);
        assert_eq!(
            Some(PathBuf::from("/var/log/oprs.log")),
            settings.logging.file
        );
        assert_eq!(LoggingLevel::Info, settings.logging.level);
        assert!(settings.targets.system);
        assert!(settings.targets.myself);
        Ok(())
    }
}
