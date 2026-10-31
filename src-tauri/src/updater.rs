use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct UpdateState(pub tauri::async_runtime::Mutex<Option<Update>>);

fn require_main(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Updates are only available from the main window.".into())
    }
}

#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, UpdateState>,
) -> Result<Option<String>, String> {
    require_main(&window)?;
    let mut pending = state
        .0
        .try_lock()
        .map_err(|_| "An update is in progress.")?;
    let update = app
        .updater_builder()
        .executable_path(crate::process_name::executable_path().map_err(|e| e.to_string())?)
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let version = update.as_ref().map(|update| update.version.clone());
    *pending = update;
    Ok(version)
}

#[derive(Clone, serde::Serialize)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, UpdateState>,
) -> Result<(), String> {
    require_main(&window)?;
    let pending = state
        .0
        .try_lock()
        .map_err(|_| "An update is in progress.")?;
    let update = pending.as_ref().ok_or("Check for an update first.")?;
    let mut downloaded = 0;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = window.emit("update-progress", Progress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;

    // Download verifies the signature before activity stops or an installer runs.
    app.state::<crate::AppState>().controller.stop(&app);
    let _ = window.emit("update-installing", ());
    let update = update.clone();
    tauri::async_runtime::spawn_blocking(move || update.install(bytes))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    app.restart();
}
