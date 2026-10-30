use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[cfg(target_os = "windows")]
mod accessibility;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
use linux as native;
#[cfg(target_os = "macos")]
use macos as native;
#[cfg(target_os = "windows")]
use windows as native;

pub const INPUT_MARKER: usize = 0x454D504C;

#[derive(Clone, Debug, Serialize)]
pub struct WindowInfo {
    pub id: u64,
    pub process_id: u32,
    pub title: String,
    pub application: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub domain: Option<String>,
    pub text: String,
    pub rows: Vec<Bounds>,
}

pub fn inspect_window(window: u64, process_id: u32) -> Result<Snapshot, String> {
    #[cfg(target_os = "windows")]
    {
        accessibility::inspect(window, process_id)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, process_id);
        Err("Accessible document capture is currently available on Windows only.".into())
    }
}

pub type Callback = Arc<dyn Fn() + Send + Sync>;
static MONITOR_STARTED: Mutex<bool> = Mutex::new(false);

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn monitor_ended() {
    if let Ok(mut started) = MONITOR_STARTED.lock() {
        *started = false;
    }
}

pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    native::list_windows()
}

pub fn foreground_window() -> Option<u64> {
    native::foreground_window()
}

pub fn focus_window(id: u64) -> Result<(), String> {
    native::focus_window(id)
}

pub fn window_bounds(id: u64) -> Option<Bounds> {
    native::window_bounds(id)
}

pub fn start_monitor(on_input: Callback, on_stop: Callback) -> Result<(), String> {
    let mut started = MONITOR_STARTED
        .lock()
        .map_err(|_| "Input monitor state unavailable")?;
    if !*started {
        native::start_monitor(on_input, on_stop)?;
        *started = true;
    }
    Ok(())
}
