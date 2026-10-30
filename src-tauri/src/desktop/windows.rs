use super::{Bounds, Callback, WindowInfo, INPUT_MARKER};
use std::{
    ffi::c_void,
    mem, ptr,
    sync::{
        atomic::{AtomicU8, Ordering},
        OnceLock,
    },
};

type Handle = *mut c_void;
#[repr(C)]
#[derive(Default)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
#[derive(Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
struct Message {
    window: Handle,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    point: Point,
    private: u32,
}
#[repr(C)]
struct KeyboardEvent {
    key: u32,
    scan: u32,
    flags: u32,
    time: u32,
    extra: usize,
}
#[repr(C)]
struct MouseEvent {
    point: Point,
    data: u32,
    flags: u32,
    time: u32,
    extra: usize,
}
#[link(name = "user32")]
extern "system" {
    fn EnumWindows(
        callback: unsafe extern "system" fn(Handle, isize) -> i32,
        parameter: isize,
    ) -> i32;
    fn IsWindowVisible(window: Handle) -> i32;
    fn IsWindow(window: Handle) -> i32;
    fn IsIconic(window: Handle) -> i32;
    fn GetWindowTextW(window: Handle, text: *mut u16, capacity: i32) -> i32;
    fn GetWindowThreadProcessId(window: Handle, pid: *mut u32) -> u32;
    fn GetForegroundWindow() -> Handle;
    fn GetAsyncKeyState(key: i32) -> i16;
    fn SetForegroundWindow(window: Handle) -> i32;
    fn ShowWindow(window: Handle, command: i32) -> i32;
    fn GetClientRect(window: Handle, rect: *mut Rect) -> i32;
    fn ClientToScreen(window: Handle, point: *mut Point) -> i32;
    fn SetWindowsHookExW(
        kind: i32,
        callback: unsafe extern "system" fn(i32, usize, isize) -> isize,
        module: Handle,
        thread: u32,
    ) -> Handle;
    fn UnhookWindowsHookEx(hook: Handle) -> i32;
    fn CallNextHookEx(hook: Handle, code: i32, wparam: usize, lparam: isize) -> isize;
    fn GetMessageW(message: *mut Message, window: Handle, minimum: u32, maximum: u32) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn QueryFullProcessImageNameW(
        process: Handle,
        flags: u32,
        name: *mut u16,
        length: *mut u32,
    ) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
    fn GetModuleHandleW(name: *const u16) -> Handle;
}

unsafe extern "system" fn collect(window: Handle, parameter: isize) -> i32 {
    if IsWindowVisible(window) == 0 {
        return 1;
    }
    let mut title = [0u16; 1024];
    let length = GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32);
    if length <= 0 {
        return 1;
    }
    let mut pid = 0;
    GetWindowThreadProcessId(window, &mut pid);
    let process = OpenProcess(0x1000, 0, pid);
    let mut application = String::new();
    if !process.is_null() {
        let mut name = [0u16; 32768];
        let mut size = name.len() as u32;
        if QueryFullProcessImageNameW(process, 0, name.as_mut_ptr(), &mut size) != 0 {
            let path = String::from_utf16_lossy(&name[..size as usize]);
            application = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_owned();
        }
        CloseHandle(process);
    }
    // EnumWindows calls synchronously while this exclusively borrowed vector lives.
    let windows = &mut *(parameter as *mut Vec<WindowInfo>);
    windows.push(WindowInfo {
        id: window as usize as u64,
        process_id: pid,
        title: String::from_utf16_lossy(&title[..length as usize]),
        application,
    });
    1
}

pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    let mut windows = Vec::<WindowInfo>::new();
    if unsafe { EnumWindows(collect, &mut windows as *mut _ as isize) } == 0 {
        return Err(format!(
            "Cannot enumerate windows: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(windows)
}
pub fn foreground_window() -> Option<u64> {
    let window = unsafe { GetForegroundWindow() };
    (!window.is_null()).then_some(window as usize as u64)
}
pub fn pointer_button_down() -> bool {
    [1, 2, 4, 5, 6]
        .iter()
        .any(|key| unsafe { GetAsyncKeyState(*key) < 0 })
}
pub fn focus_window(id: u64) -> Result<(), String> {
    let window = id as usize as Handle;
    unsafe {
        if IsWindow(window) == 0 {
            return Err("Selected window is closed".into());
        }
        if GetForegroundWindow() == window {
            return Ok(());
        }
        if IsIconic(window) != 0 {
            ShowWindow(window, 9);
        }
        if SetForegroundWindow(window) == 0 {
            return Err("Windows refused to focus the selected window".into());
        }
    }
    Ok(())
}
pub fn window_bounds(id: u64) -> Option<Bounds> {
    let mut rect = Rect::default();
    let mut point = Point::default();
    unsafe {
        if GetClientRect(id as usize as Handle, &mut rect) == 0
            || ClientToScreen(id as usize as Handle, &mut point) == 0
        {
            return None;
        }
    }
    let bounds = Bounds {
        x: point.x,
        y: point.y,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    };
    (bounds.width > 0 && bounds.height > 0).then_some(bounds)
}

static CALLBACKS: OnceLock<(Callback, Callback)> = OnceLock::new();
static MODIFIERS: AtomicU8 = AtomicU8::new(0);
unsafe extern "system" fn keyboard(code: i32, wparam: usize, lparam: isize) -> isize {
    if code >= 0 {
        let event = &*(lparam as *const KeyboardEvent);
        if event.flags & 0x10 == 0 && event.extra != INPUT_MARKER {
            let bit = match event.key {
                0x11 | 0xa2 => 1,
                0xa3 => 2,
                0x12 | 0xa4 => 4,
                0xa5 => 8,
                _ => 0,
            };
            if matches!(wparam, 0x100 | 0x104) {
                MODIFIERS.fetch_or(bit, Ordering::Relaxed);
            } else {
                MODIFIERS.fetch_and(!bit, Ordering::Relaxed);
            }
            let modifiers = MODIFIERS.load(Ordering::Relaxed);
            if let Some((input, stop)) = CALLBACKS.get() {
                if event.key == 0x1b
                    && matches!(wparam, 0x100 | 0x104)
                    && modifiers & 3 != 0
                    && modifiers & 12 != 0
                {
                    stop();
                } else {
                    input();
                }
            }
        }
    }
    CallNextHookEx(ptr::null_mut(), code, wparam, lparam)
}
unsafe extern "system" fn mouse(code: i32, wparam: usize, lparam: isize) -> isize {
    if code >= 0 {
        let event = &*(lparam as *const MouseEvent);
        if event.flags & 1 == 0 && event.extra != INPUT_MARKER {
            if let Some((input, _)) = CALLBACKS.get() {
                input();
            }
        }
    }
    CallNextHookEx(ptr::null_mut(), code, wparam, lparam)
}
pub fn start_monitor(on_input: Callback, on_stop: Callback) -> Result<(), String> {
    let _ = CALLBACKS.set((on_input, on_stop));
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("physical-input".into())
        .spawn(move || unsafe {
            let module = GetModuleHandleW(ptr::null());
            let keys = SetWindowsHookExW(13, keyboard, module, 0);
            let pointer = SetWindowsHookExW(14, mouse, module, 0);
            if keys.is_null() || pointer.is_null() {
                let error = format!(
                    "Cannot monitor user input: {}",
                    std::io::Error::last_os_error()
                );
                if !keys.is_null() {
                    UnhookWindowsHookEx(keys);
                }
                if !pointer.is_null() {
                    UnhookWindowsHookEx(pointer);
                }
                let _ = sender.send(Err(error));
                return;
            }
            let _ = sender.send(Ok(()));
            let mut message: Message = mem::zeroed();
            while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {}
            UnhookWindowsHookEx(keys);
            UnhookWindowsHookEx(pointer);
            super::monitor_ended();
            if let Some((_, stop)) = CALLBACKS.get() {
                stop();
            }
        })
        .map_err(|error| error.to_string())?;
    receiver.recv().map_err(|error| error.to_string())?
}
