use super::{Bounds, Callback, WindowInfo, INPUT_MARKER};
use std::{
    collections::HashSet,
    ffi::{c_char, c_void, CString},
    ptr,
    sync::Mutex,
};

type Object = *const c_void;
#[repr(C)]
#[derive(Default)]
struct Point {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Default)]
struct Size {
    width: f64,
    height: f64,
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRetain(object: Object) -> Object;
    fn CFRelease(object: Object);
    fn CFEqual(left: Object, right: Object) -> u8;
    fn CFStringCreateWithCString(allocator: Object, text: *const c_char, encoding: u32) -> Object;
    fn CFStringGetLength(text: Object) -> isize;
    fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    fn CFStringGetCString(text: Object, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
    fn CFArrayGetCount(array: Object) -> isize;
    fn CFArrayGetValueAtIndex(array: Object, index: isize) -> Object;
    fn CFDictionaryGetValue(dictionary: Object, key: Object) -> Object;
    fn CFNumberGetValue(number: Object, kind: i32, output: *mut c_void) -> u8;
    fn CFMachPortCreateRunLoopSource(allocator: Object, port: Object, order: isize) -> Object;
    fn CFRunLoopGetCurrent() -> Object;
    fn CFRunLoopAddSource(loop_: Object, source: Object, mode: Object);
    fn CFRunLoopRun();
    static kCFBooleanTrue: Object;
    static kCFRunLoopCommonModes: Object;
}
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> Object;
    fn AXUIElementCreateSystemWide() -> Object;
    fn AXUIElementCopyAttributeValue(element: Object, attribute: Object, value: *mut Object)
        -> i32;
    fn AXUIElementSetAttributeValue(element: Object, attribute: Object, value: Object) -> i32;
    fn AXUIElementPerformAction(element: Object, action: Object) -> i32;
    fn AXValueGetValue(value: Object, kind: i32, output: *mut c_void) -> u8;
    fn CGWindowListCopyWindowInfo(options: u32, relative: u32) -> Object;
    fn CGPreflightListenEventAccess() -> u8;
    fn CGRequestListenEventAccess() -> u8;
    fn CGEventTapCreate(
        location: u32,
        placement: u32,
        options: u32,
        mask: u64,
        callback: unsafe extern "C" fn(Object, u32, Object, *mut c_void) -> Object,
        data: *mut c_void,
    ) -> Object;
    fn CGEventTapEnable(tap: Object, enable: u8);
    fn CGEventGetIntegerValueField(event: Object, field: u32) -> i64;
    fn CGEventGetFlags(event: Object) -> u64;
    fn CGEventSourceButtonState(source: i32, button: u32) -> u8;
    static kCGWindowOwnerPID: Object;
    static kCGWindowOwnerName: Object;
}

struct Owned(Object);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CFRelease(self.0);
            }
        }
    }
}
fn string(value: &str) -> Owned {
    let text = CString::new(value).unwrap();
    Owned(unsafe { CFStringCreateWithCString(ptr::null(), text.as_ptr(), 0x08000100) })
}
fn text(value: Object) -> String {
    if value.is_null() {
        return String::new();
    }
    unsafe {
        let size = CFStringGetMaximumSizeForEncoding(CFStringGetLength(value), 0x08000100) + 1;
        if !(1..=1_048_576).contains(&size) {
            return String::new();
        }
        let mut buffer = vec![0u8; size as usize];
        if CFStringGetCString(value, buffer.as_mut_ptr().cast(), size, 0x08000100) == 0 {
            return String::new();
        }
        let length = buffer
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(buffer.len());
        String::from_utf8_lossy(&buffer[..length]).into_owned()
    }
}
fn attribute(element: Object, name: &str) -> Option<Owned> {
    let key = string(name);
    let mut value = ptr::null();
    let status = unsafe { AXUIElementCopyAttributeValue(element, key.0, &mut value) };
    (status == 0 && !value.is_null()).then_some(Owned(value))
}
struct Entry {
    element: usize,
    process_id: u32,
}
// Retained AX handles live for this process; all AX access is serialized by this mutex.
static WINDOWS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    if unsafe { AXIsProcessTrusted() } == 0 {
        return Err("Enable Accessibility access in macOS System Settings".into());
    }
    let raw = Owned(unsafe { CGWindowListCopyWindowInfo(1 | 16, 0) });
    if raw.0.is_null() {
        return Err("Cannot enumerate macOS windows".into());
    }
    let mut registry = WINDOWS.lock().map_err(|_| "Window registry unavailable")?;
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    unsafe {
        for index in 0..CFArrayGetCount(raw.0) {
            let window = CFArrayGetValueAtIndex(raw.0, index);
            let number = CFDictionaryGetValue(window, kCGWindowOwnerPID);
            let mut pid = 0i32;
            if number.is_null()
                || CFNumberGetValue(number, 3, &mut pid as *mut _ as *mut c_void) == 0
                || pid <= 0
                || !seen.insert(pid)
            {
                continue;
            }
            let application = text(CFDictionaryGetValue(window, kCGWindowOwnerName));
            let app = Owned(AXUIElementCreateApplication(pid));
            let Some(windows) = attribute(app.0, "AXWindows") else {
                continue;
            };
            for item in 0..CFArrayGetCount(windows.0) {
                let element = CFArrayGetValueAtIndex(windows.0, item);
                let Some(title) = attribute(element, "AXTitle") else {
                    continue;
                };
                let title = text(title.0);
                if title.is_empty() {
                    continue;
                }
                let existing = registry
                    .iter()
                    .position(|entry| CFEqual(entry.element as Object, element) != 0);
                let position = existing.unwrap_or_else(|| {
                    registry.push(Entry {
                        element: CFRetain(element) as usize,
                        process_id: pid as u32,
                    });
                    registry.len() - 1
                });
                result.push(WindowInfo {
                    id: position as u64 + 1,
                    process_id: registry[position].process_id,
                    title,
                    application: application.clone(),
                });
            }
        }
    }
    Ok(result)
}
pub fn foreground_window() -> Option<u64> {
    let registry = WINDOWS.lock().ok()?;
    let system = Owned(unsafe { AXUIElementCreateSystemWide() });
    let app = attribute(system.0, "AXFocusedApplication")?;
    let focused = attribute(app.0, "AXFocusedWindow")?;
    registry
        .iter()
        .position(|entry| unsafe { CFEqual(entry.element as Object, focused.0) } != 0)
        .map(|index| index as u64 + 1)
}
pub fn pointer_button_down() -> bool {
    (0..32).any(|button| unsafe { CGEventSourceButtonState(1, button) != 0 })
}
pub fn focus_window(id: u64) -> Result<(), String> {
    let registry = WINDOWS.lock().map_err(|_| "Window registry unavailable")?;
    let entry = registry
        .get(id.checked_sub(1).ok_or("Unknown window")? as usize)
        .ok_or("Unknown window")?;
    let app = Owned(unsafe { AXUIElementCreateApplication(entry.process_id as i32) });
    let frontmost = string("AXFrontmost");
    let main = string("AXMain");
    let raise = string("AXRaise");
    unsafe {
        if AXUIElementSetAttributeValue(app.0, frontmost.0, kCFBooleanTrue) != 0
            || AXUIElementSetAttributeValue(entry.element as Object, main.0, kCFBooleanTrue) != 0
            || AXUIElementPerformAction(entry.element as Object, raise.0) != 0
        {
            return Err("macOS refused to focus the selected window".into());
        }
    }
    Ok(())
}
pub fn window_bounds(id: u64) -> Option<Bounds> {
    let registry = WINDOWS.lock().ok()?;
    let entry = registry.get(id.checked_sub(1)? as usize)?;
    let position = attribute(entry.element as Object, "AXPosition")?;
    let size = attribute(entry.element as Object, "AXSize")?;
    let mut point = Point::default();
    let mut dimensions = Size::default();
    unsafe {
        if AXValueGetValue(position.0, 1, &mut point as *mut _ as *mut c_void) == 0
            || AXValueGetValue(size.0, 2, &mut dimensions as *mut _ as *mut c_void) == 0
        {
            return None;
        }
    }
    if !point.x.is_finite()
        || !point.y.is_finite()
        || !dimensions.width.is_finite()
        || !dimensions.height.is_finite()
        || dimensions.width <= 0.0
        || dimensions.height <= 40.0
    {
        return None;
    }
    // AX exposes frame geometry. Keep pointer actions below the standard title area.
    Some(Bounds {
        x: point.x as i32,
        y: point.y as i32 + 40,
        width: dimensions.width as i32,
        height: dimensions.height as i32 - 40,
    })
}

struct Monitor {
    input: Callback,
    stop: Callback,
    tap: usize,
}
unsafe extern "C" fn receive(_: Object, kind: u32, event: Object, context: *mut c_void) -> Object {
    let monitor = &*(context as *const Monitor);
    if kind == u32::MAX || kind == u32::MAX - 1 {
        (monitor.stop)();
        CGEventTapEnable(monitor.tap as Object, 1);
        return event;
    }
    if !event.is_null() && CGEventGetIntegerValueField(event, 42) != INPUT_MARKER as i64 {
        let flags = CGEventGetFlags(event);
        if kind == 10
            && CGEventGetIntegerValueField(event, 9) == 53
            && flags & (1 << 18) != 0
            && flags & (1 << 19) != 0
        {
            (monitor.stop)();
        } else {
            (monitor.input)();
        }
    }
    event
}
pub fn start_monitor(on_input: Callback, on_stop: Callback) -> Result<(), String> {
    if unsafe { CGPreflightListenEventAccess() } == 0 {
        unsafe {
            CGRequestListenEventAccess();
        }
        return Err(
            "Enable Input Monitoring access in macOS System Settings, then try Start again".into(),
        );
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("physical-input".into())
        .spawn(move || unsafe {
            let mut monitor = Box::new(Monitor {
                input: on_input,
                stop: on_stop,
                tap: 0,
            });
            let mask = [1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 22, 25, 26, 27]
                .iter()
                .fold(0u64, |mask, kind| mask | (1 << kind));
            let tap = Owned(CGEventTapCreate(
                1,
                0,
                1,
                mask,
                receive,
                (&mut *monitor as *mut Monitor).cast(),
            ));
            if tap.0.is_null() {
                let _ = sender.send(Err(
                    "macOS refused input monitoring; check Input Monitoring permission".into(),
                ));
                return;
            }
            monitor.tap = tap.0 as usize;
            let source = Owned(CFMachPortCreateRunLoopSource(ptr::null(), tap.0, 0));
            if source.0.is_null() {
                let _ = sender.send(Err("Cannot create input monitor run loop".into()));
                return;
            }
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source.0, kCFRunLoopCommonModes);
            CGEventTapEnable(tap.0, 1);
            let _ = sender.send(Ok(()));
            CFRunLoopRun();
            super::monitor_ended();
            (monitor.stop)();
        })
        .map_err(|error| error.to_string())?;
    receiver.recv().map_err(|error| error.to_string())?
}
