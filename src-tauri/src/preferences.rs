use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::schedule::Schedule;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Preferences {
    pub schedule: Schedule,
    pub autostart: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Settings {
    #[serde(default)]
    pub preferences: Preferences,
}

pub struct SettingsState {
    pub path: PathBuf,
    pub stored: Mutex<Settings>,
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default())
            }
            Err(error) => return Err(format!("Cannot load settings: {error}")),
        };
        let settings: Self =
            serde_json::from_slice(&bytes).map_err(|error| format!("Invalid settings: {error}"))?;
        settings.preferences.schedule.validate()?;
        Ok(settings)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let directory = path.parent().ok_or("Invalid settings path")?;
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let temporary = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        std::fs::write(&temporary, bytes)
            .map_err(|error| format!("Cannot save settings: {error}"))?;
        std::fs::rename(temporary, path)
            .map_err(|error| format!("Cannot replace settings: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obsolete_window_permissions_are_ignored_without_losing_preferences() {
        let stored = r#"{"preferences":{"schedule":{"start":"09:00","finish":"18:00"},"autostart":true},"targets":[{"application":"editor.exe","title":"Notes","context":"reading"}],"practice":true}"#;
        let settings: Settings = serde_json::from_str(stored).unwrap();
        assert_eq!(settings.preferences.schedule.start, "09:00");
        assert_eq!(settings.preferences.schedule.finish, "18:00");
        assert!(settings.preferences.autostart);
        let saved = serde_json::to_value(settings).unwrap();
        assert!(saved.get("targets").is_none());
        assert!(saved.get("practice").is_none());
    }
}
