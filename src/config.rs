use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
}

fn config_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "warpview")?;
    Some(dirs.config_dir().join("config.toml"))
}

pub fn load() -> Option<WindowConfig> {
    let path = config_path()?;
    let contents = std::fs::read_to_string(path).ok()?;
    toml::from_str(&contents).ok()
}

pub fn save(config: WindowConfig) {
    let Some(path) = config_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(contents) = toml::to_string_pretty(&config) {
        let _ = std::fs::write(path, contents);
    }
}
