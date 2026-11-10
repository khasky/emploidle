use std::borrow::Cow;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use chrono::{NaiveTime, Timelike};
use futures::channel::mpsc::{unbounded, UnboundedReceiver};
use futures::StreamExt;
use gpui_kit::base::{HourCycle, TimeField, TimeFieldState, TimeSegment};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::notifier::UiEvent;
use crate::preferences::{Preferences, Theme};
use crate::schedule::Schedule;
use crate::tray_status::{Command, Icons, Tray};
use crate::updater::{self, Update};
use crate::AppState;

const CHECK: &str = "check.svg";
const CLOCK: &str = "clock.svg";
const SUN: &str = "sun.svg";
const MOON: &str = "moon.svg";
const MONITOR: &str = "monitor.svg";
const GITHUB: &str = "github.svg";
const REPOSITORY: &str = "https://github.com/khasky/emploidle";
const THEMES: [(Theme, &str, &str); 3] = [
    (Theme::Light, "Light", SUN),
    (Theme::Dark, "Dark", MOON),
    (Theme::System, "System", MONITOR),
];
const DAYS: [(&str, &str); 7] = [
    ("Mon", "Monday"),
    ("Tue", "Tuesday"),
    ("Wed", "Wednesday"),
    ("Thu", "Thursday"),
    ("Fri", "Friday"),
    ("Sat", "Saturday"),
    ("Sun", "Sunday"),
];

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(match path {
            CHECK => Some(Cow::Borrowed(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 11 11"><path d="M2 5.6 4.4 8 9.2 3" fill="none" stroke="#000" stroke-width="1.9"/></svg>"##,
            )),
            CLOCK => Some(Cow::Borrowed(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.6" fill="none" stroke="#000" stroke-width="1.4"/><path d="M8 4v4.3l2.6 1.9" fill="none" stroke="#000" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
            )),
            SUN => Some(stroke_icon("M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z")),
            MOON => Some(stroke_icon("M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z")),
            MONITOR => Some(stroke_icon("M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z")),
            GITHUB => Some(Cow::Borrowed(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-2 -2 20 20" fill="#000"><path d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z"/></svg>"##,
            )),
            _ => None,
        })
    }

    fn list(&self, _: &str) -> Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

/// The theme icons of Git Account Manager.
fn stroke_icon(path: &str) -> Cow<'static, [u8]> {
    Cow::Owned(
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="{path}"/></svg>"##
        )
        .into_bytes(),
    )
}

/// The colors of the original stylesheet, including the native controls it used.
#[derive(Clone, Copy)]
struct Palette {
    dark: bool,
    bg: Hsla,
    fg: Hsla,
    muted: Hsla,
    accent: Hsla,
    stop: Hsla,
    card: Hsla,
    border: Hsla,
    check_bg: Hsla,
    check_border: Hsla,
    check_hover: Hsla,
}

impl Palette {
    fn new(theme: Theme, window: &Window) -> Self {
        let dark = match theme {
            Theme::Light => false,
            Theme::Dark => true,
            Theme::System => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        };
        let common = |bg, fg, muted, card, border, check_bg, check_border, check_hover| Self {
            dark,
            bg: rgb(bg).into(),
            fg: rgb(fg).into(),
            muted: rgb(muted).into(),
            accent: rgb(0x2d6cdf).into(),
            stop: rgb(0xd9534f).into(),
            card: rgb(card).into(),
            border: rgba(border).into(),
            check_bg: rgb(check_bg).into(),
            check_border: rgb(check_border).into(),
            check_hover: rgb(check_hover).into(),
        };
        if dark {
            common(
                0x1e1f22, 0xe8e8ea, 0x9a9ea6, 0x26272b, 0xffffff1a, 0x3b3b3b, 0x858585, 0xacacac,
            )
        } else {
            common(
                0xf4f5f7, 0x1c1c1e, 0x6b6f76, 0xffffff, 0x00000014, 0xffffff, 0x767676, 0x4f4f4f,
            )
        }
    }

    /// `filter: brightness(1.05)` on hover.
    fn brighter(color: Hsla) -> Hsla {
        let rgba = color.to_rgb();
        Rgba {
            r: (rgba.r * 1.05).min(1.),
            g: (rgba.g * 1.05).min(1.),
            b: (rgba.b * 1.05).min(1.),
            a: rgba.a,
        }
        .into()
    }
}

#[cfg(target_os = "windows")]
fn hour_cycle() -> HourCycle {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocaleInfoEx(name: *const u16, kind: u32, data: *mut u16, length: i32) -> i32;
    }
    const LOCALE_STIMEFORMAT: u32 = 0x1003;
    let mut buffer = [0u16; 80];
    // A null locale name is the user's default locale, which the WebView time input followed.
    let length = unsafe {
        GetLocaleInfoEx(
            std::ptr::null(),
            LOCALE_STIMEFORMAT,
            buffer.as_mut_ptr(),
            buffer.len() as i32,
        )
    };
    if length > 0 && !String::from_utf16_lossy(&buffer[..length as usize]).contains('H') {
        HourCycle::H12
    } else {
        HourCycle::H23
    }
}

#[cfg(not(target_os = "windows"))]
fn hour_cycle() -> HourCycle {
    HourCycle::H23
}

fn parse_time(value: &str) -> NaiveTime {
    NaiveTime::parse_from_str(value, "%H:%M").unwrap_or(NaiveTime::MIN)
}

fn format_time(time: NaiveTime) -> String {
    format!("{:02}:{:02}", time.hour(), time.minute())
}

struct Main {
    state: Arc<AppState>,
    banner: Entity<UpdateBanner>,
    view: Option<Entity<MainView>>,
    window: Option<AnyWindowHandle>,
    tray: Option<Tray>,
}

impl Global for Main {}

pub fn run(state: Arc<AppState>, mut events: UnboundedReceiver<UiEvent>, background: bool) {
    application().with_assets(Assets).run(move |cx| {
        gpui_kit::init(cx);
        let (commands, mut tray_commands) = unbounded();
        let tray = Icons::new()
            .and_then(|icons| Tray::new(&state.process_name, icons, commands))
            .inspect_err(|error| {
                state.controller.report(
                    &state.notifier,
                    &format!("Cannot create the tray icon: {error}"),
                )
            })
            .ok();
        // Without a tray icon, closing the window is the only way to quit.
        cx.set_quit_mode(if tray.is_some() {
            QuitMode::Explicit
        } else {
            QuitMode::LastWindowClosed
        });
        let banner = cx.new(|_| UpdateBanner::default());
        banner.update(cx, |banner, cx| banner.check(cx));
        cx.set_global(Main {
            state,
            banner,
            view: None,
            window: None,
            tray,
        });
        if !background || cx.global::<Main>().tray.is_none() {
            show_main(cx);
        }

        cx.spawn(async move |cx| {
            while let Some(event) = events.next().await {
                cx.update(|cx| handle_event(event, cx));
            }
        })
        .detach();
        cx.spawn(async move |cx| {
            while let Some(command) = tray_commands.next().await {
                cx.update(|cx| handle_command(command, cx));
            }
        })
        .detach();
    });
}

fn handle_event(event: UiEvent, cx: &mut App) {
    let main = cx.global_mut::<Main>();
    if let UiEvent::Tray(status) = event {
        if let Some(Err(error)) = main.tray.as_mut().map(|tray| tray.set_status(status)) {
            main.state.controller.report(
                &main.state.notifier,
                &format!("Cannot update tray icon: {error}"),
            );
        }
        return;
    }
    let Some(view) = main.view.clone() else {
        return;
    };
    view.update(cx, |view, cx| {
        match event {
            UiEvent::Status(status) => view.status = status,
            UiEvent::Running(running) => {
                view.active = running;
                if !running {
                    view.status = "Idle".into();
                }
            }
            UiEvent::Tray(_) => {}
        }
        cx.notify();
    });
}

fn handle_command(command: Command, cx: &mut App) {
    let state = cx.global::<Main>().state.clone();
    match command {
        Command::Show => show_main(cx),
        Command::Start => {
            cx.spawn(async move |cx| {
                let result = cx
                    .background_spawn(async move { state.set_running(true) })
                    .await;
                cx.update(|cx| match result {
                    Ok(()) => hide_main(cx),
                    Err(_) => show_main(cx),
                });
            })
            .detach();
        }
        Command::Stop => {
            let _ = state.set_running(false);
        }
        Command::Quit => cx.quit(),
    }
}

fn set_main_visible(visible: bool, cx: &App) {
    let state = &cx.global::<Main>().state;
    state.controller.set_main_visible(&state.notifier, visible);
}

/// Opens the main window, or focuses it when it is already open.
fn show_main(cx: &mut App) {
    if let Some(handle) = cx.global::<Main>().window {
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            set_main_visible(true, cx);
            return;
        }
    }
    let main = cx.global::<Main>();
    let state = main.state.clone();
    let banner = main.banner.clone();
    let existing = main.view.clone();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(440.), px(680.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(state.process_name.clone().into()),
            ..Default::default()
        }),
        window_min_size: Some(size(px(360.), px(420.))),
        app_id: Some("com.khasky.emploidle".into()),
        ..Default::default()
    };
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        let view =
            existing.unwrap_or_else(|| cx.new(|cx| MainView::new(state, banner, window, cx)));
        window.on_window_should_close(cx, |_, cx| {
            cx.global_mut::<Main>().window = None;
            set_main_visible(false, cx);
            true
        });
        view.update(cx, |_, cx| {
            cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    set_main_visible(true, cx);
                }
            })
            .detach();
            cx.observe_window_appearance(window, |_, _, cx| cx.notify())
                .detach();
        });
        // Minimizing hides the window to the tray, as closing does.
        #[cfg(target_os = "windows")]
        window
            .observe_window_visibility(|visibility, _, cx| {
                if visibility == WindowVisibility::Hidden {
                    cx.defer(hide_main);
                }
            })
            .detach();
        view
    });
    match opened {
        Ok((handle, view)) => {
            let main = cx.global_mut::<Main>();
            main.window = Some(handle);
            main.view = Some(view);
            set_main_visible(true, cx);
        }
        Err(error) => {
            let state = &cx.global::<Main>().state;
            state
                .controller
                .report(&state.notifier, &format!("Cannot open the window: {error}"));
        }
    }
}

fn hide_main(cx: &mut App) {
    if cx.global::<Main>().tray.is_none() {
        return;
    }
    if let Some(handle) = cx.global_mut::<Main>().window.take() {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
    set_main_visible(false, cx);
}

#[derive(Default)]
struct UpdateBanner {
    visible: bool,
    busy: bool,
    failed: bool,
    message: String,
    update: Option<Update>,
}

impl UpdateBanner {
    fn check(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            match cx.background_spawn(async { updater::check() }).await {
                Ok(Some(update)) => {
                    let _ = this.update(cx, |banner, cx| {
                        banner.message = format!("Emploidle {} is available.", update.version);
                        banner.update = Some(update);
                        banner.visible = true;
                        cx.notify();
                    });
                }
                Ok(None) => {}
                Err(error) => eprintln!("Update check failed: {error}"),
            }
        })
        .detach();
    }

    fn set_message(&mut self, message: String, cx: &mut Context<Self>) {
        self.message = message;
        cx.notify();
    }

    fn install(&mut self, state: Arc<AppState>, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(update) = self.update.clone() else {
            self.set_message("Update failed: Check for an update first.".into(), cx);
            return;
        };
        self.busy = true;
        self.set_message("Downloading update…".into(), cx);
        let (progress, mut downloads) = unbounded();
        cx.spawn(async move |this, cx| {
            let download = cx.background_spawn(async move {
                updater::download(&update, |downloaded, total| {
                    let _ = progress.unbounded_send((downloaded, total));
                })
            });
            while let Some((downloaded, total)) = downloads.next().await {
                let message = match total {
                    Some(total) if total > 0 => format!(
                        "Downloading update… {}%",
                        (downloaded as f64 / total as f64 * 100.).round().min(100.)
                    ),
                    _ => "Downloading update…".into(),
                };
                let _ = this.update(cx, |banner, cx| banner.set_message(message, cx));
            }
            let result = match download.await {
                Ok(bytes) => {
                    // The download is verified before activity stops or an installer runs.
                    state.controller.stop(&state.notifier);
                    let _ = this.update(cx, |banner, cx| {
                        banner.set_message("Installing update… Restarting Emploidle.".into(), cx)
                    });
                    cx.background_spawn(async move { updater::install(&bytes) })
                        .await
                }
                Err(error) => Err(error),
            };
            match result {
                Ok(()) => cx.update(|cx| cx.quit()),
                Err(error) => {
                    let _ = this.update(cx, |banner, cx| {
                        banner.busy = false;
                        banner.failed = true;
                        banner.set_message(format!("Update failed: {error}"), cx);
                    });
                }
            }
        })
        .detach();
    }
}

struct Focus {
    toggle: FocusHandle,
    install: FocusHandle,
    later: FocusHandle,
    days: [FocusHandle; 7],
    autostart: FocusHandle,
    apply: FocusHandle,
    themes: [FocusHandle; 3],
    github: FocusHandle,
}

pub struct MainView {
    state: Arc<AppState>,
    banner: Entity<UpdateBanner>,
    active: bool,
    pending: bool,
    status: String,
    error: String,
    start: Entity<TimeFieldState>,
    finish: Entity<TimeFieldState>,
    days: [bool; 7],
    autostart: bool,
    theme: Theme,
    focus: Focus,
}

impl MainView {
    fn new(
        state: Arc<AppState>,
        banner: Entity<UpdateBanner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&banner, |_, _, cx| cx.notify()).detach();
        let cycle = hour_cycle();
        let start = cx.new(|cx| TimeFieldState::new(window, cx).hour_cycle(cycle));
        let finish = cx.new(|cx| TimeFieldState::new(window, cx).hour_cycle(cycle));
        let mut view = Self {
            active: state.controller.running.load(Ordering::SeqCst),
            pending: false,
            status: state.controller.status(),
            error: String::new(),
            start,
            finish,
            days: [true; 7],
            autostart: false,
            theme: state.theme(),
            focus: Focus {
                toggle: cx.focus_handle(),
                install: cx.focus_handle(),
                later: cx.focus_handle(),
                days: std::array::from_fn(|_| cx.focus_handle()),
                autostart: cx.focus_handle(),
                apply: cx.focus_handle(),
                themes: std::array::from_fn(|_| cx.focus_handle()),
                github: cx.focus_handle(),
            },
            state,
            banner,
        };
        match view.state.preferences() {
            Ok(preferences) => view.show_preferences(preferences, window, cx),
            Err(error) => view.error = error,
        }
        view
    }

    fn show_preferences(
        &mut self,
        preferences: Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let schedule = preferences.schedule;
        self.start.update(cx, |field, cx| {
            field.set_time(parse_time(&schedule.start), window, cx)
        });
        self.finish.update(cx, |field, cx| {
            field.set_time(parse_time(&schedule.finish), window, cx)
        });
        self.days = schedule.days;
        self.autostart = preferences.autostart;
        cx.notify();
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        self.pending = true;
        self.error.clear();
        cx.notify();
        let value = !self.active;
        let state = self.state.clone();
        cx.spawn(async move |this, cx| {
            let running = state.clone();
            let result = cx
                .background_spawn(async move { running.set_running(value) })
                .await;
            if result.is_ok() && value {
                cx.update(hide_main);
            }
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.error = error;
                }
                this.active = state.controller.running.load(Ordering::SeqCst);
                this.status = state.controller.status();
                this.pending = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        self.pending = true;
        self.error.clear();
        cx.notify();
        let preferences = Preferences {
            schedule: Schedule {
                start: format_time(self.start.read(cx).time()),
                finish: format_time(self.finish.read(cx).time()),
                days: self.days,
            },
            autostart: self.autostart,
        };
        let state = self.state.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { state.update_preferences(preferences) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(preferences) => this.show_preferences(preferences, window, cx),
                    Err(error) => this.error = error,
                }
                this.pending = false;
                cx.notify();
            });
        })
        .detach();
    }
}

fn focus_ring(palette: &Palette, radius: f32) -> Div {
    // `outline: 2px solid var(--accent); outline-offset: 3px`
    div()
        .absolute()
        .top(px(-5.))
        .left(px(-5.))
        .right(px(-5.))
        .bottom(px(-5.))
        .border_2()
        .border_color(palette.accent)
        .rounded(px(radius + 4.))
}

fn keyboard_focused(handle: &FocusHandle, window: &Window) -> bool {
    handle.is_focused(window) && window.last_input_was_keyboard()
}

fn hint(palette: &Palette, text: &'static str) -> Div {
    div()
        .mt(px(6.))
        .text_size(px(14.))
        .line_height(relative(1.4))
        .text_color(palette.muted)
        .child(text)
}

fn secondary_button(
    id: &'static str,
    label: impl Into<SharedString>,
    palette: &Palette,
    focus: &FocusHandle,
    disabled: bool,
    window: &Window,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    bordered_button(id, label.clone(), palette, focus, disabled, window).child(label)
}

fn bordered_button(
    id: &'static str,
    label: SharedString,
    palette: &Palette,
    focus: &FocusHandle,
    disabled: bool,
    window: &Window,
) -> Stateful<Div> {
    div()
        .id(id)
        .track_focus(focus)
        .role(Role::Button)
        .aria_label(label)
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .min_w(px(44.))
        .min_h(px(44.))
        .px(px(9.))
        .py(px(6.))
        .rounded(px(6.))
        .border_1()
        .border_color(palette.border)
        .bg(palette.bg)
        .text_color(palette.fg)
        .text_size(px(16.))
        .when(disabled, |button| button.opacity(0.55))
        .when(!disabled, |button| button.cursor_pointer())
        .when(keyboard_focused(focus, window), |button| {
            button.child(focus_ring(palette, 6.))
        })
}

fn checkbox(palette: &Palette, checked: bool, accent: Option<Hsla>, group: &'static str) -> Div {
    // Native checkboxes: `accent-color` when set, otherwise the WebView default blue.
    let (fill, mark) = match accent {
        Some(accent) => (accent, white()),
        None if palette.dark => (rgb(0x99c8ff).into(), rgb(0x3b3b3b).into()),
        None => (rgb(0x0075ff).into(), white()),
    };
    div()
        .flex_none()
        .size(px(13.))
        .mt(px(3.))
        .mr(px(3.))
        .mb(px(3.))
        .ml(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(2.))
        .border_1()
        .when(checked, |checkbox| {
            checkbox
                .bg(fill)
                .border_color(fill)
                .child(svg().path(CHECK).size(px(11.)).text_color(mark))
        })
        .when(!checked, |checkbox| {
            checkbox
                .bg(palette.check_bg)
                .border_color(palette.check_border)
                .group_hover(group, |style| style.border_color(palette.check_hover))
        })
}

impl MainView {
    fn time_input(
        &self,
        id: &'static str,
        state: &Entity<TimeFieldState>,
        palette: &Palette,
        window: &Window,
        cx: &App,
    ) -> Div {
        let focused = state.read(cx).focus_handle(cx).is_focused(window);
        let accent = palette.accent;
        let field = TimeField::new(id, state)
            .disabled(self.pending)
            .flex()
            .flex_1()
            .h_full()
            .items_center()
            .render_segment(move |segment, segment_state, _, _| {
                segment
                    .px(px(1.))
                    .rounded(px(2.))
                    .when(segment_state.segment() == TimeSegment::Period, |segment| {
                        segment.ml(px(4.))
                    })
                    .when(focused && segment_state.is_selected(), |segment| {
                        segment.bg(accent).text_color(white())
                    })
                    .into_any_element()
            });
        div()
            .relative()
            .w_full()
            .h(px(44.))
            .flex()
            .items_center()
            .p(px(4.))
            .pl(px(5.))
            .rounded(px(6.))
            .border_1()
            .border_color(palette.border)
            .bg(palette.bg)
            .text_color(palette.fg)
            .child(field)
            .child(
                svg()
                    .path(CLOCK)
                    .flex_none()
                    .size(px(16.))
                    .mr(px(4.))
                    .text_color(palette.fg),
            )
            .when(focused, |input| input.child(focus_ring(palette, 6.)))
    }

    fn time_label(
        &self,
        label: &'static str,
        id: &'static str,
        state: &Entity<TimeFieldState>,
        palette: &Palette,
        window: &Window,
        cx: &App,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .flex_1()
            .min_w_0()
            .child(label)
            .child(self.time_input(id, state, palette, window, cx))
    }

    fn day_option(
        &self,
        index: usize,
        palette: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (label, name) = DAYS[index];
        let checked = self.days[index];
        let focus = &self.focus.days[index];
        let group = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"][index];
        div()
            .id(group)
            .group(group)
            .track_focus(focus)
            .role(Role::CheckBox)
            .aria_label(name)
            .aria_toggled(if checked {
                Toggled::True
            } else {
                Toggled::False
            })
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .min_w(px(44.))
            .min_h(px(44.))
            .text_size(px(14.))
            .cursor_pointer()
            .child(
                checkbox(palette, checked, Some(palette.accent), group)
                    .relative()
                    .when(keyboard_focused(focus, window), |checkbox| {
                        checkbox.child(focus_ring(palette, 2.))
                    }),
            )
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.pending {
                    this.days[index] = !this.days[index];
                    cx.notify();
                }
            }))
    }

    fn legend(palette: &Palette, text: &'static str, cut_border: bool) -> Div {
        div()
            .relative()
            .flex_none()
            .px(px(2.))
            .text_size(px(16.))
            .font_weight(FontWeight::SEMIBOLD)
            .when(cut_border, |legend| {
                // The fieldset border passes behind the legend's vertical center.
                legend.child(
                    div()
                        .absolute()
                        .top(px(10.))
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .bg(palette.card),
                )
            })
            .child(div().relative().child(text))
    }

    fn schedule(&self, palette: &Palette, window: &Window, cx: &mut Context<Self>) -> Div {
        let autostart_focus = &self.focus.autostart;
        let content =
            div()
                .flex()
                .flex_col()
                .items_start()
                .px(px(13.))
                .pt(px(12.))
                .pb(px(13.))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .gap(px(12.))
                        .child(self.time_label(
                            "Start",
                            "schedule-start",
                            &self.start,
                            palette,
                            window,
                            cx,
                        ))
                        .child(self.time_label(
                            "Finish",
                            "schedule-finish",
                            &self.finish,
                            palette,
                            window,
                            cx,
                        )),
                )
                .child(
                    div()
                        .mt(px(12.))
                        .w_full()
                        .flex()
                        .flex_col()
                        .items_start()
                        .child(Self::legend(palette, "Days", false))
                        .child(div().flex().flex_wrap().gap(px(8.)).children(
                            (0..7).map(|index| self.day_option(index, palette, window, cx)),
                        )),
                )
                .child(hint(palette, "Days apply to the start of each shift."))
                .child(
                    div()
                        .id("autostart")
                        .group("autostart")
                        .track_focus(autostart_focus)
                        .role(Role::CheckBox)
                        .aria_label("Start with system")
                        .aria_toggled(if self.autostart {
                            Toggled::True
                        } else {
                            Toggled::False
                        })
                        .mt(px(8.))
                        .min_h(px(44.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .cursor_pointer()
                        .child(
                            checkbox(palette, self.autostart, None, "autostart")
                                .relative()
                                .when(keyboard_focused(autostart_focus, window), |checkbox| {
                                    checkbox.child(focus_ring(palette, 2.))
                                }),
                        )
                        .child(div().pl(px(4.)).child("Start with system"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if !this.pending {
                                this.autostart = !this.autostart;
                                cx.notify();
                            }
                        })),
                )
                .child(hint(
                    palette,
                    "00:00–00:00 or no selected days disables schedule. Uses system time.",
                ))
                .child(hint(palette, "Schedule runs while Emploidle is open."))
                .child(hint(
                    palette,
                    "Stop pauses the current shift; the next shift starts automatically.",
                ))
                .child(
                    secondary_button(
                        "apply",
                        "Apply",
                        palette,
                        &self.focus.apply,
                        self.pending,
                        window,
                    )
                    .mt(px(12.))
                    .on_click(cx.listener(|this, _, window, cx| this.apply(window, cx))),
                );
        div()
            .relative()
            .w_full()
            .text_left()
            .child(
                div()
                    .absolute()
                    .top(px(10.6))
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.card),
            )
            .child(
                div()
                    .flex()
                    .pl(px(13.))
                    .child(Self::legend(palette, "Schedule", true)),
            )
            .child(content.relative())
    }

    fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        if self.theme == theme {
            return;
        }
        // The theme applies at once, like Git Account Manager; a failed save keeps it for this session.
        match self.state.set_theme(theme) {
            Ok(()) => self.error.clear(),
            Err(error) => self.error = error,
        }
        self.theme = theme;
        cx.notify();
    }

    fn theme_switch(&self, palette: &Palette, window: &Window, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(14.))
            .text_color(palette.muted)
            .child("Theme")
            .child(
                div()
                    .flex()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.bg)
                    .children(
                        THEMES
                            .iter()
                            .enumerate()
                            .map(|(index, &(theme, label, icon))| {
                                let selected = self.theme == theme;
                                let focus = &self.focus.themes[index];
                                let color = if selected { white() } else { palette.fg };
                                div()
                                    .id(label)
                                    .track_focus(focus)
                                    .role(Role::RadioButton)
                                    .aria_label(label)
                                    .aria_toggled(if selected {
                                        Toggled::True
                                    } else {
                                        Toggled::False
                                    })
                                    .relative()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .min_h(px(44.))
                                    .px(px(12.))
                                    .text_color(color)
                                    .cursor_pointer()
                                    .when(index == 0, |option| option.rounded_l(px(5.)))
                                    .when(index == THEMES.len() - 1, |option| {
                                        option.rounded_r(px(5.))
                                    })
                                    .when(selected, |option| option.bg(palette.accent))
                                    .when(!selected, |option| {
                                        option.hover(|style| style.bg(palette.card))
                                    })
                                    .when(keyboard_focused(focus, window), |option| {
                                        option.child(focus_ring(palette, 6.))
                                    })
                                    .child(svg().path(icon).size(px(16.)).text_color(color))
                                    .child(label)
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| {
                                            this.set_theme(theme, cx)
                                        }),
                                    )
                            }),
                    ),
            )
            .child(
                bordered_button(
                    "github",
                    "GitHub Repository".into(),
                    palette,
                    &self.focus.github,
                    false,
                    window,
                )
                .child(svg().path(GITHUB).size(px(20.)).text_color(palette.fg))
                .on_click(|_, _, cx| cx.open_url(REPOSITORY)),
            )
    }

    fn update_banner(
        &self,
        palette: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let banner = self.banner.read(cx);
        if !banner.visible {
            return None;
        }
        let busy = banner.busy;
        let label = if banner.failed {
            "Retry"
        } else {
            "Update and restart"
        };
        Some(
            div()
                .w_full()
                .p(px(12.))
                .rounded(px(10.))
                .border_1()
                .border_color(palette.accent)
                .bg(palette.card)
                .child(
                    div()
                        .id("update-message")
                        .mb(px(8.))
                        .role(Role::Status)
                        .child(banner.message.clone()),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .justify_center()
                        .gap(px(8.))
                        .child(
                            secondary_button(
                                "update-install",
                                label,
                                palette,
                                &self.focus.install,
                                busy,
                                window,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                let state = this.state.clone();
                                this.banner
                                    .update(cx, |banner, cx| banner.install(state, cx));
                            })),
                        )
                        .child(
                            secondary_button(
                                "update-later",
                                "Later",
                                palette,
                                &self.focus.later,
                                busy,
                                window,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.banner.update(cx, |banner, cx| {
                                    if !banner.busy {
                                        banner.visible = false;
                                        cx.notify();
                                    }
                                });
                            })),
                        ),
                ),
        )
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::new(self.theme, window);
        let accent = if self.active {
            palette.stop
        } else {
            palette.accent
        };
        let toggle_focus = &self.focus.toggle;
        let status = if self.status.is_empty() {
            "Idle".to_string()
        } else {
            self.status.clone()
        };
        let app = div()
            .w_full()
            .max_w(px(520.))
            .px(px(20.))
            .py(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.))
            .text_center()
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(4.6))
                    .child(
                        div()
                            .text_size(px(17.6))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Emploidle"),
                    )
                    .child(
                        div()
                            .pb(px(1.2))
                            .text_size(px(12.8))
                            .text_color(palette.muted)
                            .id("version")
                            .aria_label("Application version")
                            .child(concat!("v", env!("CARGO_PKG_VERSION"))),
                    ),
            )
            .children(self.update_banner(&palette, window, cx))
            .child(
                div()
                    .id("status")
                    .w_full()
                    .role(Role::Status)
                    .text_color(palette.muted)
                    .child(status),
            )
            .child(
                div()
                    .id("toggle")
                    .track_focus(toggle_focus)
                    .role(Role::Button)
                    .aria_label(if self.active { "Stop" } else { "Start" })
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .min_h(px(44.))
                    .min_w(px(44.))
                    .px(px(34.))
                    .py(px(12.))
                    .rounded(px(10.))
                    .bg(accent)
                    .text_color(white())
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(self.pending, |button| button.opacity(0.55))
                    .when(!self.pending, |button| {
                        button
                            .cursor_pointer()
                            .hover(|style| style.bg(Palette::brighter(accent)))
                            .active(|style| style.top(px(1.)))
                    })
                    .when(keyboard_focused(toggle_focus, window), |button| {
                        button.child(focus_ring(&palette, 10.))
                    })
                    .child(if self.active { "Stop" } else { "Start" })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(cx))),
            )
            .child(
                hint(
                    &palette,
                    "Open windows are discovered automatically. Activity adapts to the application and available controls.",
                )
                .w_full(),
            )
            .child(self.schedule(&palette, window, cx))
            .when(!self.error.is_empty(), |app| {
                app.child(
                    div()
                        .id("error")
                        .w_full()
                        .role(Role::Alert)
                        .text_size(px(14.))
                        .text_color(palette.stop)
                        .child(self.error.clone()),
                )
            })
            .child(
                hint(
                    &palette,
                    "Pauses while you use the mouse or keyboard. Ctrl + Alt + Escape stops activity globally.",
                )
                .w_full(),
            )
            .child(
                hint(
                    &palette,
                    "Closing the window hides it to the tray. Quit from the tray menu.",
                )
                .w_full(),
            )
            .child(self.theme_switch(&palette, window, cx));
        div()
            .id("page")
            .size_full()
            .overflow_y_scroll()
            .bg(palette.bg)
            .text_color(palette.fg)
            .font_family(".SystemUIFont")
            .text_size(px(16.))
            .line_height(relative(1.33))
            .flex()
            .justify_center()
            .items_start()
            .child(app)
    }
}

/// Regenerates `screenshots/` with demonstration data: `pnpm screenshots`.
/// Nothing here reads open windows, changes startup settings or runs activity.
#[cfg(test)]
mod screenshots {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::Duration;

    use image::{imageops, Rgba, RgbaImage};

    use super::*;
    use crate::activity::Controller;
    use crate::notifier::Notifier;
    use crate::preferences::{Settings, SettingsState};

    #[derive(Clone, Copy, PartialEq)]
    enum Scene {
        Idle,
        Paused,
        Update,
    }

    fn open(theme: Theme, scene: Scene, cx: &mut App) -> WindowHandle<MainView> {
        let state = Arc::new(AppState {
            controller: Arc::new(Controller::new()),
            notifier: Notifier::new().0,
            settings: SettingsState {
                path: std::env::temp_dir().join("emploidle-screenshots.json"),
                stored: Mutex::new(Settings::default()),
            },
            process_name: "Emploidle screenshots".into(),
        });
        let banner = cx.new(|_| UpdateBanner::default());
        if scene == Scene::Update {
            let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
            banner.update(cx, |banner, _| {
                banner.message = format!(
                    "Emploidle {}.{}.{} is available.",
                    current.major,
                    current.minor,
                    current.patch + 1
                );
                banner.visible = true;
            });
        }
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(0.), px(0.)),
                size(px(440.), px(1000.)),
            ))),
            // Hidden windows do not render a frame to capture.
            focus: false,
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            cx.new(|cx| {
                let mut view = MainView::new(state, banner, window, cx);
                view.show_preferences(
                    Preferences {
                        schedule: Schedule {
                            start: "09:00".into(),
                            finish: "18:00".into(),
                            days: [true, true, true, true, true, false, false],
                        },
                        autostart: false,
                    },
                    window,
                    cx,
                );
                view.error.clear();
                view.theme = theme;
                view.active = scene == Scene::Paused;
                view.status = if view.active { "Paused" } else { "Idle" }.into();
                view
            })
        })
        .unwrap()
    }

    /// The page ends 24px below its last control, like the document did.
    fn page(image: &RgbaImage, scale: f32) -> RgbaImage {
        let background = *image.get_pixel(0, image.height() - 1);
        let last = (0..image.height())
            .rev()
            .find(|&y| (0..image.width()).any(|x| *image.get_pixel(x, y) != background))
            .unwrap();
        let height = (last + 1 + (24. * scale).round() as u32).min(image.height());
        imageops::crop_imm(image, 0, 0, image.width(), height).to_image()
    }

    /// The schedule fieldset: its card plus the legend above the card's top border.
    fn schedule(image: &RgbaImage, card: Rgba<u8>, scale: f32) -> RgbaImage {
        let rows: Vec<u32> = (0..image.height())
            .filter(|&y| {
                (0..image.width())
                    .filter(|&x| *image.get_pixel(x, y) == card)
                    .count()
                    > image.width() as usize / 2
            })
            .collect();
        let (top, bottom) = (rows[0], *rows.last().unwrap());
        let columns: Vec<u32> = (0..image.width())
            .filter(|&x| *image.get_pixel(x, top + 2) == card)
            .collect();
        let border = scale.round() as u32;
        let left = columns[0] - border;
        let right = *columns.last().unwrap() + border;
        let top = top - border - (10.6 * scale).round() as u32;
        imageops::crop_imm(
            image,
            left,
            top,
            right - left + 1,
            bottom + border - top + 1,
        )
        .to_image()
    }

    fn save(image: &RgbaImage, path: PathBuf) {
        image.save(&path).unwrap();
        println!("{} {}x{}", path.display(), image.width(), image.height());
    }

    #[std::prelude::rust_2021::test]
    #[ignore = "writes screenshots/; run with `pnpm screenshots`"]
    fn capture() {
        let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("screenshots");
        application().with_assets(Assets).run(move |cx| {
            gpui_kit::init(cx);
            cx.set_quit_mode(QuitMode::Explicit);
            cx.spawn(async move |cx| {
                for (theme, name, card) in [
                    (Theme::Light, "light", Rgba([0xff, 0xff, 0xff, 0xff])),
                    (Theme::Dark, "dark", Rgba([0x26, 0x27, 0x2b, 0xff])),
                ] {
                    for (scene, file) in [
                        (Scene::Idle, "main"),
                        (Scene::Paused, "paused"),
                        (Scene::Update, "update"),
                    ] {
                        let window = cx.update(|cx| open(theme, scene, cx));
                        cx.background_executor()
                            .timer(Duration::from_millis(700))
                            .await;
                        let image = window
                            .update(cx, |_, window, _| {
                                let image = window.render_to_image();
                                window.remove_window();
                                image
                            })
                            .unwrap()
                            .unwrap();
                        let scale = image.width() as f32 / 440.;
                        save(
                            &page(&image, scale),
                            output.join(format!("{name}-{file}.png")),
                        );
                        if scene == Scene::Idle {
                            save(
                                &schedule(&image, card, scale),
                                output.join(format!("{name}-schedule.png")),
                            );
                        }
                    }
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    }
}
