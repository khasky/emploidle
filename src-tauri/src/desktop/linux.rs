use super::{Bounds, Callback, WindowInfo};
use std::{
    cell::Cell,
    collections::HashSet,
    ffi::{c_char, c_int, c_long, c_uchar, c_ulong, c_void, CStr, CString},
    ptr,
    sync::OnceLock,
};

type Display = c_void;
type Window = c_ulong;
type ErrorHandler = unsafe extern "C" fn(*mut Display, *mut c_void) -> c_int;
thread_local! {
    static OUR_DISPLAY: Cell<*mut Display> = const { Cell::new(ptr::null_mut()) };
    static ERROR_OCCURRED: Cell<bool> = const { Cell::new(false) };
}
static PREVIOUS_HANDLER: OnceLock<Option<ErrorHandler>> = OnceLock::new();
unsafe extern "C" fn handle_error(display: *mut Display, error: *mut c_void) -> c_int {
    // A selected window can close between two requests. Preserve other clients' handlers.
    if OUR_DISPLAY.with(|current| current.get() == display) {
        ERROR_OCCURRED.with(|error| error.set(true));
        return 0;
    }
    PREVIOUS_HANDLER
        .get()
        .copied()
        .flatten()
        .map_or(0, |previous| previous(display, error))
}
#[repr(C)]
struct Cookie {
    kind: c_int,
    serial: c_ulong,
    sent: c_int,
    display: *mut Display,
    extension: c_int,
    event: c_int,
    cookie: u32,
    data: *mut c_void,
}
#[repr(C)]
struct RawEvent {
    kind: c_int,
    serial: c_ulong,
    sent: c_int,
    display: *mut Display,
    extension: c_int,
    event: c_int,
    time: c_ulong,
    device: c_int,
    source: c_int,
    detail: c_int,
}
#[repr(C)]
struct Device {
    id: c_int,
    name: *mut c_char,
    usage: c_int,
    attachment: c_int,
    enabled: c_int,
    classes: c_int,
    class_info: *mut c_void,
}
#[repr(C)]
struct EventMask {
    device: c_int,
    length: c_int,
    mask: *mut c_uchar,
}
#[repr(C)]
struct ClientEvent {
    kind: c_int,
    serial: c_ulong,
    sent: c_int,
    display: *mut Display,
    window: Window,
    message: c_ulong,
    format: c_int,
    data: [c_long; 5],
}
#[link(name = "X11")]
extern "C" {
    fn XSetErrorHandler(handler: Option<ErrorHandler>) -> Option<ErrorHandler>;
    fn XOpenDisplay(name: *const c_char) -> *mut Display;
    fn XCloseDisplay(display: *mut Display) -> c_int;
    fn XDefaultRootWindow(display: *mut Display) -> Window;
    fn XInternAtom(display: *mut Display, name: *const c_char, only_exists: c_int) -> c_ulong;
    fn XGetWindowProperty(
        display: *mut Display,
        window: Window,
        property: c_ulong,
        offset: c_long,
        length: c_long,
        delete: c_int,
        requested: c_ulong,
        actual: *mut c_ulong,
        format: *mut c_int,
        count: *mut c_ulong,
        remaining: *mut c_ulong,
        data: *mut *mut c_uchar,
    ) -> c_int;
    fn XFree(data: *mut c_void) -> c_int;
    fn XGetGeometry(
        display: *mut Display,
        window: Window,
        root: *mut Window,
        x: *mut c_int,
        y: *mut c_int,
        width: *mut u32,
        height: *mut u32,
        border: *mut u32,
        depth: *mut u32,
    ) -> c_int;
    fn XTranslateCoordinates(
        display: *mut Display,
        source: Window,
        destination: Window,
        x: c_int,
        y: c_int,
        target_x: *mut c_int,
        target_y: *mut c_int,
        child: *mut Window,
    ) -> c_int;
    fn XSendEvent(
        display: *mut Display,
        window: Window,
        propagate: c_int,
        mask: c_long,
        event: *mut c_void,
    ) -> c_int;
    fn XFlush(display: *mut Display) -> c_int;
    fn XSync(display: *mut Display, discard: c_int) -> c_int;
    fn XQueryExtension(
        display: *mut Display,
        name: *const c_char,
        opcode: *mut c_int,
        event: *mut c_int,
        error: *mut c_int,
    ) -> c_int;
    fn XNextEvent(display: *mut Display, event: *mut c_void) -> c_int;
    fn XGetEventData(display: *mut Display, cookie: *mut Cookie) -> c_int;
    fn XFreeEventData(display: *mut Display, cookie: *mut Cookie);
    fn XKeysymToKeycode(display: *mut Display, symbol: c_ulong) -> u8;
}
#[link(name = "Xi")]
extern "C" {
    fn XIQueryVersion(display: *mut Display, major: *mut c_int, minor: *mut c_int) -> c_int;
    fn XISelectEvents(
        display: *mut Display,
        window: Window,
        masks: *mut EventMask,
        count: c_int,
    ) -> c_int;
    fn XIQueryDevice(display: *mut Display, id: c_int, count: *mut c_int) -> *mut Device;
    fn XIFreeDeviceInfo(info: *mut Device);
}

struct Connection(*mut Display);
impl Connection {
    fn open() -> Result<Self, String> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|kind| kind == "wayland")
        {
            return Err("Window control and physical input monitoring require an X11 session; Wayland is unsupported".into());
        }
        let display = unsafe { XOpenDisplay(ptr::null()) };
        if display.is_null() {
            Err("Cannot connect to the X11 display".into())
        } else {
            PREVIOUS_HANDLER.get_or_init(|| unsafe { XSetErrorHandler(Some(handle_error)) });
            OUR_DISPLAY.with(|current| current.set(display));
            Ok(Self(display))
        }
    }
    fn atom(&self, name: &str) -> c_ulong {
        unsafe { XInternAtom(self.0, CString::new(name).unwrap().as_ptr(), 0) }
    }
    fn root(&self) -> Window {
        unsafe { XDefaultRootWindow(self.0) }
    }
    fn property(&self, window: Window, name: &str) -> Option<Property> {
        let (mut actual, mut format, mut count, mut remaining, mut data) =
            (0, 0, 0, 0, ptr::null_mut());
        let result = unsafe {
            XGetWindowProperty(
                self.0,
                window,
                self.atom(name),
                0,
                65536,
                0,
                0,
                &mut actual,
                &mut format,
                &mut count,
                &mut remaining,
                &mut data,
            )
        };
        if result != 0 || data.is_null() {
            None
        } else {
            Some(Property {
                data,
                format,
                count: count as usize,
            })
        }
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        unsafe {
            XCloseDisplay(self.0);
        }
        OUR_DISPLAY.with(|current| current.set(ptr::null_mut()));
    }
}
struct Property {
    data: *mut c_uchar,
    format: c_int,
    count: usize,
}
impl Property {
    fn longs(&self) -> &[c_ulong] {
        if self.format == 32 {
            unsafe { std::slice::from_raw_parts(self.data.cast(), self.count) }
        } else {
            &[]
        }
    }
    fn text(&self) -> String {
        if self.format == 8 {
            String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(self.data, self.count) })
                .trim_end_matches('\0')
                .to_owned()
        } else {
            String::new()
        }
    }
}
impl Drop for Property {
    fn drop(&mut self) {
        unsafe {
            XFree(self.data.cast());
        }
    }
}

pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    let connection = Connection::open()?;
    let clients = connection
        .property(connection.root(), "_NET_CLIENT_LIST")
        .ok_or("Window manager does not expose its window list")?;
    let mut result = Vec::new();
    for &id in clients.longs() {
        let title = connection
            .property(id, "_NET_WM_NAME")
            .or_else(|| connection.property(id, "WM_NAME"))
            .map(|p| p.text())
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        let application = connection
            .property(id, "WM_CLASS")
            .map(|p| {
                p.text()
                    .split('\0')
                    .next_back()
                    .unwrap_or_default()
                    .to_owned()
            })
            .unwrap_or_default();
        let process_id = connection
            .property(id, "_NET_WM_PID")
            .and_then(|p| p.longs().first().copied())
            .and_then(|pid| u32::try_from(pid).ok())
            .unwrap_or(0);
        result.push(WindowInfo {
            id,
            process_id,
            title,
            application,
        });
    }
    Ok(result)
}
pub fn foreground_window() -> Option<u64> {
    let connection = Connection::open().ok()?;
    connection
        .property(connection.root(), "_NET_ACTIVE_WINDOW")?
        .longs()
        .first()
        .copied()
        .filter(|id| *id != 0)
}
pub fn focus_window(id: u64) -> Result<(), String> {
    let connection = Connection::open()?;
    let event = ClientEvent {
        kind: 33,
        serial: 0,
        sent: 1,
        display: connection.0,
        window: id,
        message: connection.atom("_NET_ACTIVE_WINDOW"),
        format: 32,
        data: [1, 0, 0, 0, 0],
    };
    // XSendEvent reads a full XEvent union, so give it correctly sized storage.
    let mut storage = [0 as c_long; 24];
    unsafe {
        ptr::write(storage.as_mut_ptr().cast::<ClientEvent>(), event);
    }
    let result = unsafe {
        XSendEvent(
            connection.0,
            connection.root(),
            0,
            (1 << 20) | (1 << 19),
            storage.as_mut_ptr().cast(),
        )
    };
    unsafe {
        XFlush(connection.0);
    }
    if result == 0 {
        Err("X11 refused the focus request".into())
    } else {
        Ok(())
    }
}
pub fn window_bounds(id: u64) -> Option<Bounds> {
    let connection = Connection::open().ok()?;
    let (mut root, mut child, mut x, mut y, mut width, mut height, mut border, mut depth) =
        (0, 0, 0, 0, 0, 0, 0, 0);
    unsafe {
        if XGetGeometry(
            connection.0,
            id,
            &mut root,
            &mut x,
            &mut y,
            &mut width,
            &mut height,
            &mut border,
            &mut depth,
        ) == 0
        {
            return None;
        }
        if XTranslateCoordinates(
            connection.0,
            id,
            connection.root(),
            0,
            0,
            &mut x,
            &mut y,
            &mut child,
        ) == 0
        {
            return None;
        }
    }
    (width > 0 && height > 0).then_some(Bounds {
        x,
        y,
        width: i32::try_from(width).ok()?,
        height: i32::try_from(height).ok()?,
    })
}

unsafe fn xtest_devices(display: *mut Display) -> HashSet<c_int> {
    let mut count = 0;
    let devices = XIQueryDevice(display, 0, &mut count);
    if devices.is_null() {
        return HashSet::new();
    }
    let mut ids = HashSet::new();
    for info in std::slice::from_raw_parts(devices, count.max(0) as usize) {
        if !info.name.is_null()
            && CStr::from_ptr(info.name)
                .to_bytes()
                .windows(5)
                .any(|part| part == b"XTEST")
        {
            ids.insert(info.id);
        }
    }
    XIFreeDeviceInfo(devices);
    ids
}
pub fn start_monitor(on_input: Callback, on_stop: Callback) -> Result<(), String> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("physical-input".into())
        .spawn(move || {
            let connection = match Connection::open() {
                Ok(c) => c,
                Err(error) => {
                    let _ = sender.send(Err(error));
                    return;
                }
            };
            unsafe {
                let (mut opcode, mut event, mut error, mut major, mut minor) = (0, 0, 0, 2, 0);
                if XQueryExtension(
                    connection.0,
                    c"XInputExtension".as_ptr(),
                    &mut opcode,
                    &mut event,
                    &mut error,
                ) == 0
                    || XIQueryVersion(connection.0, &mut major, &mut minor) != 0
                {
                    let _ = sender.send(Err(
                        "XInput 2 is required for physical input monitoring".into()
                    ));
                    return;
                }
                let mut bits = [0u8; 4];
                for kind in [13, 14, 15, 16, 17] {
                    bits[kind / 8] |= 1 << (kind % 8);
                }
                let raw_mask = EventMask {
                    device: 1,
                    length: bits.len() as i32,
                    mask: bits.as_mut_ptr(),
                };
                let mut hierarchy = [0u8, 1 << 3];
                let mut masks = [
                    raw_mask,
                    EventMask {
                        device: 0,
                        length: hierarchy.len() as i32,
                        mask: hierarchy.as_mut_ptr(),
                    },
                ];
                ERROR_OCCURRED.with(|error| error.set(false));
                let selected =
                    XISelectEvents(connection.0, connection.root(), masks.as_mut_ptr(), 2);
                XSync(connection.0, 0);
                if selected != 0 || ERROR_OCCURRED.with(|error| error.get()) {
                    let _ = sender.send(Err("Cannot monitor XInput events".into()));
                    return;
                }
                XFlush(connection.0);
                let mut ignored = xtest_devices(connection.0);
                if ignored.is_empty() {
                    let _ = sender.send(Err(
                        "Cannot identify XTEST devices; refusing ambiguous input monitoring".into(),
                    ));
                    return;
                }
                let escape = XKeysymToKeycode(connection.0, 0xff1b) as c_int;
                let controls = [
                    XKeysymToKeycode(connection.0, 0xffe3) as c_int,
                    XKeysymToKeycode(connection.0, 0xffe4) as c_int,
                ];
                let alts = [
                    XKeysymToKeycode(connection.0, 0xffe9) as c_int,
                    XKeysymToKeycode(connection.0, 0xffea) as c_int,
                ];
                let mut pressed = HashSet::new();
                let _ = sender.send(Ok(()));
                loop {
                    let mut storage = [0 as c_long; 24];
                    XNextEvent(connection.0, storage.as_mut_ptr().cast());
                    let cookie = &mut *storage.as_mut_ptr().cast::<Cookie>();
                    if cookie.kind != 35
                        || cookie.extension != opcode
                        || XGetEventData(connection.0, cookie) == 0
                    {
                        continue;
                    }
                    if cookie.event == 11 {
                        ignored = xtest_devices(connection.0);
                    } else if (13..=17).contains(&cookie.event) && !cookie.data.is_null() {
                        let raw = &*cookie.data.cast::<RawEvent>();
                        if !ignored.contains(&raw.source) {
                            if cookie.event == 13 {
                                pressed.insert(raw.detail);
                            }
                            if cookie.event == 14 {
                                pressed.remove(&raw.detail);
                            }
                            if cookie.event == 13
                                && raw.detail == escape
                                && controls.iter().any(|key| pressed.contains(key))
                                && alts.iter().any(|key| pressed.contains(key))
                            {
                                on_stop();
                            } else {
                                on_input();
                            }
                        }
                    }
                    XFreeEventData(connection.0, cookie);
                }
            }
        })
        .map_err(|error| error.to_string())?;
    receiver.recv().map_err(|error| error.to_string())?
}
