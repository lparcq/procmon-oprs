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

use std::path::{Path, PathBuf};

pub const KEY_APP_NAME: &str = "name";
pub const KEY_COUNT: &str = "count";
pub const KEY_EVERY: &str = "every";
pub const KEY_HUMAN_FORMAT: &str = "human";

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

/// Set a boolean value if not yet in the config
pub fn provide(config: &mut config::Config, key: &str, value: bool) -> anyhow::Result<()> {
    if config.get_bool(key).is_err() {
        config.set(key, value)?;
    }
    Ok(())
}
