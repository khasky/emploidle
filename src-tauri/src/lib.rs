mod activity;
mod activity_plan;
mod autostart;
mod desktop;
mod preferences;
mod process_name;
mod schedule;
mod source_context;
mod updater;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, State, WindowEvent};

use activity::Controller;
use preferences::{Preferences, Settings, SettingsState};
use schedule::{Decision, LocalClock};

pub struct AppState {
    controller: Arc<Controller>,
}

/// Set the activity flag and notify the UI so the button stays in sync whether
/// the toggle came from the window or the tray menu.
fn set_running(app: &AppHandle, value: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    if value {
        state.controller.start(app).inspect_err(|error| {
            state.controller.report(app, error);
        })?;
        if let Some(window) = app.get_webview_window("main") {
            if let Err(error) = window.hide() {
                state.controller.stop(app);
                return Err(error.to_string());
            }
        }
        state.controller.set_main_visible(app, false);
        Ok(())
    } else {
        state.controller.stop(app);
        Ok(())
    }
}

#[tauri::command]
fn set_active(app: AppHandle, active: bool) -> Result<(), String> {
    set_running(&app, active)
}

#[tauri::command]
fn is_active(state: State<AppState>) -> bool {
    state.controller.running.load(Ordering::SeqCst)
}

#[tauri::command]
fn activity_status(state: State<AppState>) -> String {
    state.controller.status()
}

#[tauri::command]
fn get_preferences(app: AppHandle) -> Result<Preferences, String> {
    let mut preferences = app
        .state::<SettingsState>()
        .stored
        .lock()
        .unwrap()
        .preferences
        .clone();
    preferences.autostart = autostart::enabled()?;
    Ok(preferences)
}

#[tauri::command]
async fn update_preferences(
    app: AppHandle,
    preferences: Preferences,
) -> Result<Preferences, String> {
    tauri::async_runtime::spawn_blocking(move || {
        preferences.schedule.validate()?;
        let settings = app.state::<SettingsState>();
        let mut stored = settings.stored.lock().unwrap();
        let previous_autostart = autostart::enabled()?;
        let executable = process_name::executable_path().map_err(|error| error.to_string())?;
        if preferences.autostart != previous_autostart {
            autostart::set_enabled(preferences.autostart, &executable)?;
        }
        let mut updated = stored.clone();
        updated.preferences = preferences.clone();
        if let Err(error) = updated.save(&settings.path) {
            if preferences.autostart != previous_autostart {
                if let Err(rollback) = autostart::set_enabled(previous_autostart, &executable) {
                    return Err(format!(
                        "{error}; startup registration could not be restored: {rollback}"
                    ));
                }
            }
            return Err(error);
        }
        *stored = updated;
        app.state::<AppState>()
            .controller
            .update_schedule(preferences.schedule.clone());
        Ok(preferences)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn run() {
    let process_name = match process_name::prepare() {
        Ok(Some(name)) => name,
        Ok(None) => return,
        Err(error) => {
            eprintln!("Emploidle: cannot randomize process name: {error}");
            std::process::exit(1);
        }
    };

    let mut context = tauri::generate_context!();
    let background = std::env::args_os().any(|argument| argument == "--background");
    for window in &mut context.config_mut().app.windows {
        if window.label == "main" {
            window.title.clone_from(&process_name);
            window.visible = !background;
        }
    }

    let controller = Arc::new(Controller::new());
    controller.main_visible.store(!background, Ordering::SeqCst);

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::UpdateState::default())
        .manage(AppState { controller })
        .invoke_handler(tauri::generate_handler![
            set_active,
            is_active,
            app_version,
            activity_status,
            get_preferences,
            update_preferences,
            updater::check_update,
            updater::install_update
        ])
        .setup(move |app| {
            let path = app.path().app_config_dir()?.join("settings.json");
            let (settings, load_error) = match Settings::load(&path) {
                Ok(settings) => (settings, None),
                Err(error) => (Settings::default(), Some(error)),
            };
            let controller = app.state::<AppState>().controller.clone();
            controller
                .scheduler
                .lock()
                .unwrap()
                .update(settings.preferences.schedule.clone());
            app.manage(SettingsState {
                path,
                stored: std::sync::Mutex::new(settings),
            });
            if let Some(error) = load_error {
                controller.report(app.handle(), &error);
            }
            activity::spawn_worker(controller.clone(), app.handle().clone());
            spawn_scheduler(app.handle().clone());
            let show = MenuItem::with_id(app, "show", "Show window", true, None::<&str>)?;
            let start = MenuItem::with_id(app, "start", "Start", true, None::<&str>)?;
            let stop = MenuItem::with_id(app, "stop", "Stop", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &start, &stop, &quit])?;

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip(&process_name)
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        app.state::<AppState>()
                            .controller
                            .set_main_visible(app, true);
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.unminimize();
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "start" => {
                        if set_running(app, true).is_err() {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.unminimize();
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                    "stop" => {
                        let _ = set_running(app, false);
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            let app = window.app_handle();
            let controller = &app.state::<AppState>().controller;
            if let WindowEvent::Focused(true) = event {
                if window.is_visible().unwrap_or(false) {
                    controller.set_main_visible(app, true);
                }
            }
            if let WindowEvent::Resized(_) = event {
                if window.is_minimized().unwrap_or(false) && window.hide().is_ok() {
                    controller.set_main_visible(app, false);
                }
            }
            // Closing the window hides it to the tray; real exit is tray → Quit.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if window.hide().is_ok() {
                    controller.set_main_visible(app, false);
                }
            }
        })
        .run(context)
        .expect("error while running Emploidle");
}

fn spawn_scheduler(app: AppHandle) {
    std::thread::spawn(move || loop {
        let controller = app.state::<AppState>().controller.clone();
        if let Ok(clock) = LocalClock::now() {
            let (intent, decision) = controller.schedule_decision(clock);
            match decision {
                Some(Decision::Start) if !controller.running.load(Ordering::SeqCst) => {
                    let result = controller.start_scheduled(&app, intent);
                    if let Err(error) = result {
                        controller.fail_scheduled(&app, intent, &error);
                    }
                }
                Some(Decision::Stop) => controller.stop_for_schedule(&app, intent),
                _ => {}
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    });
}
