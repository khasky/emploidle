use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::activity::{Configuration, Target, TargetContext};
use crate::desktop::WindowInfo;
use crate::schedule::Schedule;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Preferences {
    pub schedule: Schedule,
    pub autostart: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SavedTarget {
    application: String,
    title: String,
    context: TargetContext,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Settings {
    #[serde(default)]
    pub preferences: Preferences,
    #[serde(default)]
    targets: Vec<SavedTarget>,
    #[serde(default)]
    practice: bool,
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

    pub fn remember(
        &mut self,
        configuration: &Configuration,
        windows: &[WindowInfo],
    ) -> Result<(), String> {
        let mut targets = Vec::new();
        for target in &configuration.targets {
            let source = windows
                .iter()
                .find(|window| window.id == target.id && window.process_id == target.process_id)
                .ok_or("A selected window closed before its settings could be saved.")?;
            if source.application.is_empty() {
                return Err("The selected application's identity is unavailable; its window cannot be remembered.".into());
            }
            targets.push(SavedTarget {
                application: source.application.clone(),
                title: source.title.clone(),
                context: target.context,
            });
        }
        self.targets = targets;
        self.practice = configuration.practice;
        Ok(())
    }

    pub fn resolve(&self, windows: &[WindowInfo]) -> Configuration {
        let targets = windows
            .iter()
            .filter_map(|window| {
                if window.process_id == std::process::id() {
                    return None;
                }
                self.targets
                    .iter()
                    .find(|target| {
                        target.application == window.application && target.title == window.title
                    })
                    .map(|target| Target {
                        id: window.id,
                        process_id: window.process_id,
                        context: target.context,
                    })
            })
            .collect();
        Configuration {
            targets,
            practice: self.practice,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_permissions_require_the_same_application_and_window_title() {
        let settings = Settings {
            targets: vec![SavedTarget {
                application: "notepad.exe".into(),
                title: "Notes".into(),
                context: TargetContext::Reading,
            }],
            practice: true,
            ..Settings::default()
        };
        let source = WindowInfo {
            id: 42,
            process_id: u32::MAX,
            application: "notepad.exe".into(),
            title: "Notes".into(),
        };
        let mut renamed = source.clone();
        renamed.title = "Another document".into();
        let mut different = source.clone();
        different.application = "other.exe".into();
        let mut own = source.clone();
        own.process_id = std::process::id();
        let restored = settings.resolve(&[source, renamed, different, own]);
        assert_eq!(restored.targets.len(), 1);
        assert_eq!(restored.targets[0].id, 42);
        assert!(restored.practice);
    }
}
