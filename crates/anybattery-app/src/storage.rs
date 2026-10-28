use anybattery_core::protocol::ProtocolVerification;
use anybattery_core::{Settings, UserProfileCatalog, UserProtocolProfile};
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
    match serde_json::from_str::<Settings>(&text) {
        Ok(mut settings) => {
            settings.validate();
            settings
        }
        Err(err) => {
            let backup =
                path.with_extension(format!("json.broken-{}", anybattery_core::now_unix()));
            let _ = std::fs::copy(&path, &backup);
            tracing::warn!(
                path = %path.display(),
                backup = %backup.display(),
                %err,
                "settings.json 损坏，已备份并使用默认值"
            );
            Settings::default()
        }
    }
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
    write_atomic(&path, &text).is_ok()
}

/// User protocol profiles live beside settings. The directory is created
/// lazily; a damaged file is quarantined instead of blocking the rest.
#[derive(Clone, Debug, Default)]
pub struct ProfileStore {
    dir: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct ProfileLoad {
    pub catalog: UserProfileCatalog,
    pub diagnostics: Vec<String>,
}

impl ProfileStore {
    pub fn new() -> Self {
        Self {
            dir: data_dir().join("protocols"),
        }
    }

    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path_for(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    pub fn load(&self) -> ProfileLoad {
        let mut diagnostics = Vec::new();
        if let Err(err) = std::fs::create_dir_all(&self.dir) {
            diagnostics.push(format!(
                "无法创建用户配置目录 {}：{err}",
                self.dir.display()
            ));
            return ProfileLoad {
                catalog: UserProfileCatalog::default(),
                diagnostics,
            };
        }

        let mut profiles = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            diagnostics.push(format!("无法读取用户配置目录 {}", self.dir.display()));
            return ProfileLoad {
                catalog: UserProfileCatalog::default(),
                diagnostics,
            };
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(err) => {
                    diagnostics.push(format!("无法读取用户配置 {}：{err}", path.display()));
                    continue;
                }
            };
            match UserProtocolProfile::from_json(&text) {
                Ok(profile) => {
                    if !seen_ids.insert(profile.spec.id.clone()) {
                        diagnostics.push(format!("用户配置 id 重复，已隔离：{}", path.display()));
                        quarantine(&path);
                        continue;
                    }
                    profiles.push(profile);
                }
                Err(err) => {
                    diagnostics.push(format!("用户配置无效，已隔离：{}（{err}）", path.display()));
                    quarantine(&path);
                }
            }
        }

        let catalog = match UserProfileCatalog::new(profiles.clone()) {
            Ok(catalog) => catalog,
            Err(err) => {
                diagnostics.push(format!("用户配置存在冲突，已全部禁用：{err}"));
                for profile in &mut profiles {
                    profile.enabled = false;
                    // Persist the safe state, otherwise the same conflict would
                    // come back on every launch and the UI could not resolve it.
                    if let Ok(text) = serde_json::to_string_pretty(profile) {
                        let _ = write_atomic(&self.path_for(&profile.spec.id), &text);
                    }
                }
                UserProfileCatalog::new(profiles).unwrap_or_default()
            }
        };

        ProfileLoad {
            catalog,
            diagnostics,
        }
    }

    pub fn save(&self, profile: &UserProtocolProfile) -> Result<(), String> {
        profile
            .validate()
            .map_err(|err| format!("用户配置校验失败：{err}"))?;
        std::fs::create_dir_all(&self.dir).map_err(|err| format!("无法创建用户配置目录：{err}"))?;
        let text = serde_json::to_string_pretty(profile)
            .map_err(|err| format!("无法序列化用户配置：{err}"))?;
        write_atomic(&self.path_for(&profile.spec.id), &text)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        let path = self.path_for(id);
        let text =
            std::fs::read_to_string(&path).map_err(|err| format!("无法读取用户配置：{err}"))?;
        let mut profile =
            UserProtocolProfile::from_json(&text).map_err(|err| format!("用户配置无效：{err}"))?;
        profile.enabled = enabled;
        self.save(&profile)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let path = self.path_for(id);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("无法删除用户配置：{err}")),
        }
    }

    #[allow(dead_code)] // reserved for the export button; profiles are plain JSON.
    pub fn export(&self, id: &str) -> Result<String, String> {
        std::fs::read_to_string(self.path_for(id)).map_err(|err| format!("无法导出用户配置：{err}"))
    }

    /// Imported profiles are always disabled and unverified; the UI must make
    /// the user rebind and preview before enabling.
    pub fn import(&self, text: &str) -> Result<UserProtocolProfile, String> {
        let mut profile: UserProtocolProfile =
            serde_json::from_str(text).map_err(|err| format!("无法解析用户配置：{err}"))?;
        profile.enabled = false;
        profile.spec.verification = ProtocolVerification::Unverified;
        profile
            .validate()
            .map_err(|err| format!("用户配置未通过安全校验：{err}"))?;
        self.save(&profile)?;
        Ok(profile)
    }
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Err("用户配置路径没有父目录".to_owned());
    };
    std::fs::create_dir_all(parent).map_err(|err| format!("无法创建用户配置目录：{err}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|err| format!("无法写入用户配置：{err}"))?;
    std::fs::rename(&tmp, path).map_err(|err| format!("无法保存用户配置：{err}"))?;
    Ok(())
}

fn quarantine(path: &Path) {
    let backup = path.with_extension(format!("json.broken-{}", anybattery_core::now_unix()));
    let _ = std::fs::rename(path, &backup);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(tag: &str) -> ProfileStore {
        let dir = std::env::temp_dir().join(format!(
            "anybattery-profile-{}-{}-{tag}",
            std::process::id(),
            anybattery_core::now_unix()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        ProfileStore::at(dir)
    }

    fn profile_json(id: &str) -> String {
        serde_json::json!({
            "schema": 1,
            "enabled": true,
            "binding": {
                "container_id": "container-a",
                "instance_id": "instance-a",
                "interface_number": 2,
                "usage_page": 65285,
                "usage": 1,
                "vendor_id": 4660,
                "product_id": 22136,
                "input_len": 16
            },
            "spec": {
                "schema": 1,
                "id": id,
                "name": "用户配置：示例",
                "match": {
                    "vid": "1234",
                    "pid": "5678",
                    "usage_page": "FF05",
                    "usage": "0001",
                    "min_output_len": null
                },
                "request": {
                    "kind": "none",
                    "report_id": null,
                    "length": null,
                    "payload_hex": "",
                    "pad": null
                },
                "response": {
                    "kind": "input",
                    "timeout_ms": 600,
                    "retries": 0,
                    "match": [[1, "72"]]
                },
                "extract": {
                    "level": {
                        "offset": 4,
                        "type": "u8",
                        "scale": 1.0,
                        "bias": 0.0,
                        "min": 0.0,
                        "max": 100.0
                    },
                    "charging": null,
                    "voltage_mv": null
                },
                "verification": "unverified",
                "notes": null
            }
        })
        .to_string()
    }

    #[test]
    fn save_and_load_round_trip() {
        let store = temp_store("roundtrip");
        let profile = UserProtocolProfile::from_json(&profile_json("user.example")).unwrap();
        store.save(&profile).unwrap();
        let loaded = store.load();
        assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
        assert_eq!(loaded.catalog.profiles().len(), 1);
        assert_eq!(loaded.catalog.profiles()[0].spec.id, "user.example");
        let _ = std::fs::remove_dir_all(store.dir());
    }

    #[test]
    fn corrupt_profile_is_quarantined() {
        let store = temp_store("corrupt");
        std::fs::create_dir_all(store.dir()).unwrap();
        std::fs::write(store.path_for("user.broken"), "{").unwrap();
        let loaded = store.load();
        assert!(loaded.catalog.profiles().is_empty());
        assert_eq!(loaded.diagnostics.len(), 1);
        assert!(!store.path_for("user.broken").exists());
        let _ = std::fs::remove_dir_all(store.dir());
    }

    #[test]
    fn conflicting_profiles_are_disabled_and_persisted() {
        let store = temp_store("conflict");
        let first = UserProtocolProfile::from_json(&profile_json("user.one")).unwrap();
        let mut second = UserProtocolProfile::from_json(&profile_json("user.two")).unwrap();
        second.spec.name = "第二个配置".to_owned();
        store.save(&first).unwrap();
        store.save(&second).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.catalog.profiles().len(), 2);
        assert_eq!(loaded.catalog.enabled_profiles().count(), 0);
        assert!(loaded.diagnostics.iter().any(|line| line.contains("冲突")));
        for id in ["user.one", "user.two"] {
            let text = std::fs::read_to_string(store.path_for(id)).unwrap();
            let profile = UserProtocolProfile::from_json(&text).unwrap();
            assert!(!profile.enabled, "{id} must be disabled on disk");
        }
        let _ = std::fs::remove_dir_all(store.dir());
    }

    #[test]
    fn import_forces_disabled_and_unverified() {
        let store = temp_store("import");
        let mut value: serde_json::Value =
            serde_json::from_str(&profile_json("user.imported")).unwrap();
        value["spec"]["verification"] = serde_json::json!("verified");
        let profile = store.import(&value.to_string()).unwrap();
        assert!(!profile.enabled);
        assert_eq!(
            profile.spec.verification,
            anybattery_core::protocol::ProtocolVerification::Unverified
        );
        let loaded = store.load();
        assert_eq!(loaded.catalog.profiles().len(), 1);
        assert!(!loaded.catalog.profiles()[0].enabled);
        let _ = std::fs::remove_dir_all(store.dir());
    }
}
