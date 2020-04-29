pub const KEY_APP_NAME: &str = "name";
pub const KEY_COUNT: &str = "count";
pub const KEY_EVERY: &str = "every";
pub const KEY_HUMAN_FORMAT: &str = "human";

pub struct Reader {
    xdg_dirs: xdg::BaseDirectories,
}

impl Reader {
    pub fn new(app_name: &str) -> anyhow::Result<Reader> {
        Ok(Reader {
            xdg_dirs: xdg::BaseDirectories::with_prefix(app_name)?,
        })
    }

    /// Read config file searching for extension .toml, .yaml, .json.
    pub fn read_config_file(&self, config: &mut config::Config, name: &str) -> anyhow::Result<()> {
        for extension in &["toml", "yaml", "json"] {
            let basename = format!("{}.{}", name, extension);
            for config_file_name in self.xdg_dirs.find_config_files(basename) {
                if config_file_name.exists() {
                    let config_file = config::File::from(config_file_name);
                    config.merge(config_file)?;
                    return Ok(());
                }
            }
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
