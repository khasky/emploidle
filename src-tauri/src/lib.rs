mod activity;
mod process_name;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

pub struct AppState {
    running: Arc<AtomicBool>,
}

/// Set the activity flag and notify the UI so the button stays in sync whether
/// the toggle came from the window or the tray menu.
fn set_running(app: &AppHandle, value: bool) {
    app.state::<AppState>().running.store(value, Ordering::Relaxed);
    let _ = app.emit("running-changed", value);
}

#[tauri::command]
fn set_active(app: AppHandle, active: bool) {
    set_running(&app, active);
}

#[tauri::command]
fn is_active(state: State<AppState>) -> bool {
    state.running.load(Ordering::Relaxed)
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
    for window in &mut context.config_mut().app.windows {
        window.title.clone_from(&process_name);
    }

    let running = Arc::new(AtomicBool::new(false));
    activity::spawn_worker(running.clone());

    tauri::Builder::default()
        .manage(AppState { running })
        .invoke_handler(tauri::generate_handler![set_active, is_active, app_version])
        .setup(move |app| {
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
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "start" => set_running(app, true),
                    "stop" => set_running(app, false),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window hides it to the tray; real exit is tray → Quit.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(context)
        .expect("error while running Emploidle");
}
