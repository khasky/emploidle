mod activity;
mod activity_plan;
mod autostart;
mod desktop;
mod notifier;
mod preferences;
mod process_name;
mod schedule;
mod source_context;
mod tray_status;
mod ui;
mod updater;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use activity::Controller;
use notifier::Notifier;
use preferences::{Preferences, Settings, SettingsState, Theme};
use schedule::{Decision, LocalClock};

pub struct AppState {
    controller: Arc<Controller>,
    notifier: Notifier,
    settings: SettingsState,
    process_name: String,
}

impl AppState {
    fn preferences(&self) -> Result<Preferences, String> {
        let mut preferences = self.settings.stored.lock().unwrap().preferences.clone();
        preferences.autostart = autostart::enabled()?;
        Ok(preferences)
    }

    fn update_preferences(&self, preferences: Preferences) -> Result<Preferences, String> {
        preferences.schedule.validate()?;
        let mut stored = self.settings.stored.lock().unwrap();
        let previous_autostart = autostart::enabled()?;
        let executable = process_name::executable_path().map_err(|error| error.to_string())?;
        if preferences.autostart != previous_autostart {
            autostart::set_enabled(preferences.autostart, &executable)?;
        }
        let mut updated = stored.clone();
        updated.preferences = preferences.clone();
        if let Err(error) = updated.save(&self.settings.path) {
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
        self.controller
            .update_schedule(preferences.schedule.clone());
        Ok(preferences)
    }

    fn theme(&self) -> Theme {
        self.settings.stored.lock().unwrap().theme
    }

    fn set_theme(&self, theme: Theme) -> Result<(), String> {
        let mut stored = self.settings.stored.lock().unwrap();
        let mut updated = stored.clone();
        updated.theme = theme;
        updated.save(&self.settings.path)?;
        *stored = updated;
        Ok(())
    }

    /// Starts or stops activity; the caller hides the main window after a start.
    fn set_running(&self, value: bool) -> Result<(), String> {
        if value {
            self.controller
                .start(&self.notifier)
                .inspect_err(|error| self.controller.report(&self.notifier, error))
        } else {
            self.controller.stop(&self.notifier);
            Ok(())
        }
    }
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
    let background = std::env::args_os().any(|argument| argument == "--background");

    let path = dirs::config_dir()
        .unwrap_or_default()
        .join("com.khasky.emploidle")
        .join("settings.json");
    let (settings, load_error) = match Settings::load(&path) {
        Ok(settings) => (settings, None),
        Err(error) => (Settings::default(), Some(error)),
    };
    let controller = Arc::new(Controller::new());
    controller.main_visible.store(!background, Ordering::SeqCst);
    controller
        .scheduler
        .lock()
        .unwrap()
        .update(settings.preferences.schedule.clone());
    let (notifier, events) = Notifier::new();
    if let Some(error) = load_error {
        controller.report(&notifier, &error);
    }
    activity::spawn_worker(controller.clone(), notifier.clone());
    spawn_scheduler(controller.clone(), notifier.clone());
    let state = Arc::new(AppState {
        controller,
        notifier,
        settings: SettingsState {
            path,
            stored: Mutex::new(settings),
        },
        process_name,
    });
    ui::run(state, events, background);
}

fn spawn_scheduler(controller: Arc<Controller>, app: Notifier) {
    std::thread::spawn(move || loop {
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
