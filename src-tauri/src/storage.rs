use std::{fs, path::Path};
use serde::{Deserialize, Serialize};
use crate::session::{GameMode, Settings};

#[derive(Serialize, Deserialize)]
struct Preferences { version: u32, settings: Settings }

pub fn load(directory: &Path) -> Settings {
    let mut settings = fs::read(directory.join("settings.json")).ok()
        .and_then(|data| serde_json::from_slice::<Preferences>(&data).ok())
        .filter(|value| value.version == 1 && value.settings.validate().is_ok())
        .map(|value| value.settings).unwrap_or_default();
    // A stored preference cannot restore the lifetime of a network connection.
    if settings.mode == GameMode::Lan { settings.mode = GameMode::Computer; }
    settings
}

pub fn save(directory: &Path, settings: &Settings) -> Result<(), String> {
    settings.validate()?;
    fs::create_dir_all(directory).map_err(|error| format!("无法创建设置目录：{error}"))?;
    let bytes = serde_json::to_vec_pretty(&Preferences { version: 1, settings: settings.clone() })
        .map_err(|error| format!("无法保存设置：{error}"))?;
    let temporary = directory.join("settings.json.tmp");
    fs::write(&temporary, bytes).map_err(|error| format!("无法写入设置：{error}"))?;
    fs::rename(temporary, directory.join("settings.json")).map_err(|error| format!("无法更新设置：{error}"))
}
