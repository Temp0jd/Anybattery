use anybattery_core::Settings;
use std::path::{Path, PathBuf};

pub fn data_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("portable.txt").exists() {
                return dir.join("data");
            }
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("AnyBattery");
    }
    PathBuf::from(".")
}

pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn load() -> Settings {
    let path = settings_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Settings::default();
    };
    let mut settings: Settings = serde_json::from_str(&text).unwrap_or_default();
    settings.validate();
    settings
}

pub fn save(settings: &Settings) -> bool {
    let path = settings_path();
    let Some(parent) = path.parent() else {
        return false;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    let Ok(text) = serde_json::to_string_pretty(settings) else {
        return false;
    };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_err() {
        return false;
    }
    std::fs::rename(tmp, path).is_ok()
}

pub fn clamp_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}
