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
    pub rows: Vec<Bounds>,
    #[serde(default)]
    pub scroll: Option<ScrollPosition>,
    #[serde(default)]
    pub links: Vec<AccessibleLink>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccessibleLink {
    pub address: String,
    pub visited: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BrowserContext {
    pub address: String,
    pub selected: Vec<i32>,
    pub address_focused: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrowserSession {
    pub created: Vec<i32>,
    pub original: Vec<i32>,
}

pub fn browser_context(window: u64, process_id: u32) -> Result<BrowserContext, String> {
    #[cfg(target_os = "windows")]
    {
        accessibility::browser_context(window, process_id)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, process_id);
        Err("Browser controls are unavailable on this platform.".into())
    }
}

pub fn open_browser_tab(window: u64, process_id: u32) -> Result<BrowserSession, String> {
    #[cfg(target_os = "windows")]
    {
        accessibility::open_tab(window, process_id)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, process_id);
        Err("Browser controls are unavailable on this platform.".into())
    }
}

pub fn close_browser_tab(
    window: u64,
    process_id: u32,
    session: BrowserSession,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        accessibility::close_tab(window, process_id, session)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, process_id, session);
        Err("Browser controls are unavailable on this platform.".into())
    }
}

pub fn review_tree(window: u64, process_id: u32, index: u32) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        accessibility::review_tree(window, process_id, index)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, process_id, index);
        Ok(false)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScrollPosition {
    pub element: Vec<i32>,
    pub horizontal: f64,
    pub vertical: f64,
    pub bounds: Bounds,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScrollReceipt {
    pub before: ScrollPosition,
    pub after: ScrollPosition,
}

pub fn scroll_window(
    window: u64,
    process_id: u32,
    horizontal: bool,
    amount: i32,
) -> Result<ScrollReceipt, String> {
    #[cfg(target_os = "windows")]
    {
        accessibility::scroll(window, process_id, horizontal, amount)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, process_id, horizontal, amount);
        Err("This platform does not expose restorable scrolling.".into())
    }
}

pub fn restore_scroll(window: u64, process_id: u32, receipt: ScrollReceipt) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        accessibility::restore(window, process_id, receipt)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, process_id, receipt);
        Err("This platform does not expose restorable scrolling.".into())
    }
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

pub fn pointer_button_down() -> bool {
    native::pointer_button_down()
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
