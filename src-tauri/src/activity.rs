use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use rand::Rng;
use tauri::{AppHandle, Emitter};

use crate::activity_plan::{self, Action, Plan};
use crate::desktop::{self, Bounds};
use crate::schedule::{Decision, LocalClock, Schedule, Scheduler};
use crate::source_context::SourceKind;

const USER_PAUSE: Duration = Duration::from_secs(8);

pub struct Controller {
    pub running: AtomicBool,
    status: Mutex<String>,
    input_epoch: AtomicU64,
    last_input: Mutex<Instant>,
    input_paused: AtomicBool,
    stopped_by_user: AtomicBool,
    pub main_visible: AtomicBool,
    pub scheduler: Mutex<Scheduler>,
    pub intent_epoch: AtomicU64,
    intent_lock: Mutex<()>,
}

impl Controller {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            status: Mutex::new("Idle".into()),
            input_epoch: AtomicU64::new(0),
            last_input: Mutex::new(Instant::now()),
            input_paused: AtomicBool::new(false),
            stopped_by_user: AtomicBool::new(false),
            main_visible: AtomicBool::new(false),
            scheduler: Mutex::new(Scheduler::new(Schedule::default())),
            intent_epoch: AtomicU64::new(0),
            intent_lock: Mutex::new(()),
        }
    }

    pub fn status(&self) -> String {
        self.status.lock().unwrap().clone()
    }

    pub fn report(&self, app: &AppHandle, message: &str) {
        let mut status = self.status.lock().unwrap();
        if *status != message {
            *status = message.into();
            let _ = app.emit("activity-status", message);
        }
    }

    pub fn stop(&self, app: &AppHandle) {
        let _intent = self.intent_lock.lock().unwrap();
        self.stopped_by_user.store(true, Ordering::SeqCst);
        self.intent_epoch.fetch_add(1, Ordering::SeqCst);
        if let Ok(clock) = LocalClock::now() {
            self.scheduler.lock().unwrap().manual_stop(clock);
        }
        self.stop_activity(app);
    }

    pub fn stop_for_schedule(&self, app: &AppHandle, expected_intent: u64) {
        let _intent = self.intent_lock.lock().unwrap();
        if self.intent_epoch.load(Ordering::SeqCst) == expected_intent {
            self.stopped_by_user.store(false, Ordering::SeqCst);
            self.stop_activity(app);
        }
    }

    pub fn fail_scheduled(&self, app: &AppHandle, expected_intent: u64, error: &str) {
        let _intent = self.intent_lock.lock().unwrap();
        if self.intent_epoch.load(Ordering::SeqCst) != expected_intent {
            return;
        }
        if let Ok(clock) = LocalClock::now() {
            self.scheduler.lock().unwrap().manual_stop(clock);
        }
        self.stop_activity(app);
        self.report(app, error);
    }

    pub fn schedule_decision(&self, clock: LocalClock) -> (u64, Option<Decision>) {
        let _intent = self.intent_lock.lock().unwrap();
        (
            self.intent_epoch.load(Ordering::SeqCst),
            self.scheduler.lock().unwrap().tick(clock),
        )
    }

    fn fail_current(&self, app: &AppHandle, epoch: u64, error: &str) {
        let _intent = self.intent_lock.lock().unwrap();
        if !self.session_valid(epoch) {
            return;
        }
        self.intent_epoch.fetch_add(1, Ordering::SeqCst);
        if let Ok(clock) = LocalClock::now() {
            self.scheduler.lock().unwrap().manual_stop(clock);
        }
        self.stop_activity(app);
        self.report(app, error);
    }

    fn stop_activity(&self, app: &AppHandle) {
        self.running.store(false, Ordering::SeqCst);
        self.input_epoch.fetch_add(1, Ordering::SeqCst);
        let _ = app.emit("running-changed", false);
        self.report(app, "Idle");
        self.refresh_tray(app);
    }

    fn user_input(&self) {
        *self.last_input.lock().unwrap() = Instant::now();
        self.input_paused.store(true, Ordering::SeqCst);
        self.input_epoch.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn tray_status(&self) -> crate::tray_status::Status {
        use crate::tray_status::Status;
        if !self.running.load(Ordering::SeqCst) {
            return if self.stopped_by_user.load(Ordering::SeqCst) {
                Status::Paused
            } else {
                Status::Idle
            };
        }
        if self.main_visible.load(Ordering::SeqCst)
            || (self.input_paused.load(Ordering::SeqCst)
                && self.last_input.lock().unwrap().elapsed() < USER_PAUSE)
        {
            Status::Paused
        } else {
            Status::Active
        }
    }

    fn refresh_tray(&self, app: &AppHandle) {
        if let Err(error) = crate::tray_status::update(app, self.tray_status()) {
            self.report(app, &format!("Cannot update tray icon: {error}"));
        }
    }

    pub fn set_main_visible(&self, app: &AppHandle, visible: bool) {
        self.main_visible.store(visible, Ordering::SeqCst);
        self.input_epoch.fetch_add(1, Ordering::SeqCst);
        self.refresh_tray(app);
        if self.running.load(Ordering::SeqCst) {
            self.report(
                app,
                if visible {
                    "Paused"
                } else {
                    "Resuming activity..."
                },
            );
        }
    }

    pub fn update_schedule(&self, schedule: Schedule) {
        let _intent = self.intent_lock.lock().unwrap();
        let mut scheduler = self.scheduler.lock().unwrap();
        if scheduler.update(schedule) {
            self.intent_epoch.fetch_add(1, Ordering::SeqCst);
            if self.running.load(Ordering::SeqCst) {
                if let Ok(clock) = LocalClock::now() {
                    scheduler.manual_start(clock);
                }
            }
        }
    }

    fn session_valid(&self, epoch: u64) -> bool {
        self.running.load(Ordering::SeqCst)
            && !self.main_visible.load(Ordering::SeqCst)
            && self.input_epoch.load(Ordering::SeqCst) == epoch
    }

    pub fn start(self: &Arc<Self>, app: &AppHandle) -> Result<(), String> {
        let _intent = self.intent_lock.lock().unwrap();
        self.intent_epoch.fetch_add(1, Ordering::SeqCst);
        let clock = LocalClock::now()?;
        self.activate(app)?;
        self.scheduler.lock().unwrap().manual_start(clock);
        Ok(())
    }

    pub fn start_scheduled(
        self: &Arc<Self>,
        app: &AppHandle,
        expected_intent: u64,
    ) -> Result<(), String> {
        let _intent = self.intent_lock.lock().unwrap();
        if self.intent_epoch.load(Ordering::SeqCst) != expected_intent {
            return Ok(());
        }
        self.activate(app)
    }

    fn activate(self: &Arc<Self>, app: &AppHandle) -> Result<(), String> {
        desktop::list_windows()?;
        let input_controller = self.clone();
        let stop_controller = self.clone();
        let stop_app = app.clone();
        desktop::start_monitor(
            Arc::new(move || input_controller.user_input()),
            Arc::new(move || stop_controller.stop(&stop_app)),
        )?;
        self.user_input();
        self.input_paused.store(false, Ordering::SeqCst);
        self.stopped_by_user.store(false, Ordering::SeqCst);
        self.running.store(true, Ordering::SeqCst);
        self.refresh_tray(app);
        self.report(
            app,
            if self.main_visible.load(Ordering::SeqCst) {
                "Paused"
            } else {
                "Waiting for user input to settle..."
            },
        );
        let _ = app.emit("running-changed", true);
        Ok(())
    }
}

fn automatic_targets(windows: &[desktop::WindowInfo]) -> Vec<u64> {
    windows
        .iter()
        .filter(|window| window.process_id != std::process::id())
        .map(|window| window.id)
        .collect()
}

#[derive(Debug)]
enum StepError {
    Interrupted,
    Failed(String),
    RestorationFailed(String),
}

type StepResult = Result<(), StepError>;

struct Guard<'a> {
    controller: &'a Controller,
    epoch: u64,
    window: u64,
}

impl Guard<'_> {
    fn check_input(&self) -> StepResult {
        if !self.controller.running.load(Ordering::SeqCst)
            || self.controller.main_visible.load(Ordering::SeqCst)
            || self.controller.input_epoch.load(Ordering::SeqCst) != self.epoch
        {
            return Err(StepError::Interrupted);
        }
        Ok(())
    }

    fn check(&self) -> StepResult {
        self.check_input()?;
        self.check_pointer()?;
        if desktop::foreground_window() != Some(self.window) {
            return Err(StepError::Failed(
                "The window lost focus. Start again when ready.".into(),
            ));
        }
        Ok(())
    }

    fn focus(&self) -> StepResult {
        self.check_input()?;
        self.check_pointer()?;
        if desktop::foreground_window() != Some(self.window) {
            desktop::focus_window(self.window).map_err(StepError::Failed)?;
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            self.check_input()?;
            if desktop::foreground_window() == Some(self.window) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(StepError::Failed("The window could not be focused.".into()));
            }
            thread::sleep(Duration::from_millis(30));
        }
    }

    fn wait(&self, duration: Duration) -> StepResult {
        let deadline = Instant::now() + duration;
        loop {
            self.check()?;
            let now = Instant::now();
            if now >= deadline {
                return Ok(());
            }
            thread::sleep((deadline - now).min(Duration::from_millis(30)));
        }
    }

    fn input(&self, operation: impl FnOnce() -> enigo::InputResult<()>) -> StepResult {
        self.check()?;
        operation().map_err(|error| StepError::Failed(error.to_string()))
    }

    fn check_pointer(&self) -> StepResult {
        if desktop::pointer_button_down() {
            self.controller.user_input();
            return Err(StepError::Interrupted);
        }
        Ok(())
    }
}

fn smooth_move(e: &mut Enigo, guard: &Guard<'_>, bounds: Bounds, rng: &mut impl Rng) -> StepResult {
    let margin = 24;
    if bounds.width < 120 || bounds.height < 120 {
        return Err(StepError::Failed(
            "The window is too small. Enlarge it and start again.".into(),
        ));
    }
    let x = rng.random_range(bounds.x + margin..bounds.x + bounds.width - margin);
    let top = 60;
    let y = rng.random_range(bounds.y + top..bounds.y + bounds.height - margin);
    smooth_point(e, guard, (x, y), rng)
}

fn smooth_point(
    e: &mut Enigo,
    guard: &Guard<'_>,
    (x, y): (i32, i32),
    rng: &mut impl Rng,
) -> StepResult {
    let start = e
        .location()
        .map_err(|error| StepError::Failed(error.to_string()))?;
    let end = (x, y);
    let distance = f64::from(end.0 - start.0).hypot(f64::from(end.1 - start.1));
    let duration = (120.0 + distance * 0.25).clamp(130.0, 450.0) as u64;
    let steps = (duration / 10) as usize;
    let path = activity_plan::pointer_path(start, end, rng.random_range(-0.18..=0.18), steps);
    for point in path {
        guard.input(|| e.move_mouse(point.0, point.1, Coordinate::Abs))?;
        guard.wait(Duration::from_millis(rng.random_range(8..=12)))?;
    }
    if rng.random_bool(0.25) {
        guard.wait(Duration::from_millis(rng.random_range(35..=90)))?;
        guard.input(|| {
            e.move_mouse(
                x + rng.random_range(-3..=3),
                y + rng.random_range(-2..=2),
                Coordinate::Abs,
            )
        })?;
    }
    Ok(())
}

fn external_action(
    action: Action,
    e: &mut Enigo,
    guard: &Guard<'_>,
    source: &desktop::WindowInfo,
    snapshot: Option<&desktop::Snapshot>,
    rng: &mut impl Rng,
) -> StepResult {
    guard.check()?;
    if action == Action::Pause {
        return Ok(());
    }
    let bounds = desktop::window_bounds(source.id)
        .ok_or_else(|| StepError::Failed("The window is no longer available.".into()))?;
    if let Action::Scroll(amount) | Action::HorizontalScroll(amount) = action {
        let kind = SourceKind::detect(
            snapshot.and_then(|snapshot| snapshot.domain.as_deref()),
            &source.application,
        );
        if let Some(scroll) = snapshot
            .and_then(|snapshot| snapshot.scroll.as_ref())
            .filter(|scroll| {
                kind.can_scroll()
                    && if matches!(action, Action::HorizontalScroll(_)) {
                        scroll.horizontal >= 0.0
                    } else {
                        scroll.vertical >= 0.0
                    }
            })
        {
            smooth_move(e, guard, scroll.bounds, rng)?;
            let receipt = desktop::scroll_window(
                source.id,
                source.process_id,
                matches!(action, Action::HorizontalScroll(_)),
                amount,
            )
            .map_err(StepError::RestorationFailed)?;
            let reading = guard.wait(Duration::from_millis(rng.random_range(2_000..=6_000)));
            let restored =
                desktop::restore_scroll(source.id, source.process_id, receipt).map_err(|error| {
                    if error.contains("preserving the current view") {
                        StepError::Interrupted
                    } else {
                        StepError::RestorationFailed(error)
                    }
                });
            return restored.and(reading);
        }
    }
    let rows: Vec<_> = snapshot
        .into_iter()
        .flat_map(|snapshot| &snapshot.rows)
        .filter(|row| {
            row.width > 16
                && row.height > 4
                && row.x >= bounds.x + 16
                && row.y >= bounds.y + 24
                && row.x.saturating_add(row.width) <= bounds.x.saturating_add(bounds.width) - 16
                && row.y.saturating_add(row.height) <= bounds.y.saturating_add(bounds.height) - 16
        })
        .copied()
        .collect();
    if bounds.width < 120 || bounds.height < 120 {
        return Err(StepError::Failed(
            "The window is too small for cursor activity.".into(),
        ));
    }
    for point in activity_plan::pointer_route(bounds, &rows, rng) {
        smooth_point(e, guard, point, rng)?;
        guard.wait(Duration::from_millis(rng.random_range(45..=150)))?;
    }
    Ok(())
}

fn review_editor(
    e: &mut Enigo,
    guard: &Guard<'_>,
    source: &desktop::WindowInfo,
    rng: &mut impl Rng,
    action: u32,
    exclude_files: &[String],
) -> Result<Option<String>, StepError> {
    guard.check()?;
    let Some(mut session) = desktop::begin_editor_review(
        source.id,
        source.process_id,
        action + 3 * rng.random_range(0..100_000),
        exclude_files,
    )
    .map_err(StepError::RestorationFailed)?
    else {
        return Ok(None);
    };
    let reviewed_file = session.file.as_ref().map(|file| file.name.clone());
    let result = (|| {
        if let Some(file) = &session.file {
            let bounds = file.bounds;
            if let Some(window) = desktop::window_bounds(source.id) {
                // Leave an existing Explorer tooltip before approaching the next file.
                smooth_point(
                    e,
                    guard,
                    (
                        window.x + window.width * 3 / 4,
                        window.y + window.height / 2,
                    ),
                    rng,
                )?;
            }
            smooth_point(
                e,
                guard,
                (bounds.x + bounds.width / 2, bounds.y + bounds.height / 2),
                rng,
            )?;
            let fresh = desktop::editor_file_target(source.id, source.process_id, session.clone())
                .map_err(StepError::Failed)?;
            if (fresh.x, fresh.y, fresh.width, fresh.height)
                != (bounds.x, bounds.y, bounds.width, bounds.height)
            {
                return Err(StepError::Interrupted);
            }
            guard.input(|| e.button(Button::Middle, Direction::Click))?;
            session = desktop::capture_editor_review(source.id, source.process_id, session.clone())
                .map_err(StepError::RestorationFailed)?;
            guard.check()?;
            let viewport = desktop::editor_viewport(source.id, source.process_id, session.clone())
                .map_err(StepError::Failed)?;
            smooth_point(
                e,
                guard,
                (
                    viewport.x + viewport.width / 2,
                    viewport.y + viewport.height / 2,
                ),
                rng,
            )?;
            for _ in 0..rng.random_range(5..=10) {
                let fresh = desktop::editor_viewport(source.id, source.process_id, session.clone())
                    .map_err(StepError::Failed)?;
                if (fresh.x, fresh.y, fresh.width, fresh.height)
                    != (viewport.x, viewport.y, viewport.width, viewport.height)
                {
                    return Err(StepError::Interrupted);
                }
                guard.input(|| e.scroll(1, Axis::Vertical))?;
                guard.wait(Duration::from_millis(rng.random_range(100..=220)))?;
            }
            guard.check()?;
        }
        guard.wait(Duration::from_millis(rng.random_range(300..=650)))?;
        if action != 2 {
            if let Some(bounds) = desktop::window_bounds(source.id) {
                smooth_point(
                    e,
                    guard,
                    (bounds.x + bounds.width / 3, bounds.y + bounds.height / 3),
                    rng,
                )?;
            }
        }
        guard.wait(Duration::from_millis(rng.random_range(400..=900)))
    })();
    let interrupted = !guard.controller.session_valid(guard.epoch);
    let restored =
        desktop::restore_editor_review(source.id, source.process_id, session, interrupted)
            .map_err(StepError::RestorationFailed);
    restored.and(result).map(|()| reviewed_file)
}

fn review_editor_sequence(
    e: &mut Enigo,
    guard: &Guard<'_>,
    source: &desktop::WindowInfo,
    rng: &mut impl Rng,
    app: &AppHandle,
) -> StepResult {
    guard
        .controller
        .report(app, "VS Code: reviewing Extensions");
    review_editor(e, guard, source, rng, 0, &[])?;
    guard.check()?;
    guard.controller.report(app, "VS Code: reviewing Explorer");
    let explorer = desktop::begin_editor_review(source.id, source.process_id, 1, &[])
        .map_err(StepError::RestorationFailed)?;
    let result = (|| {
        guard.wait(Duration::from_millis(rng.random_range(450..=850)))?;
        desktop::review_tree(source.id, source.process_id, rng.random())
            .map_err(StepError::RestorationFailed)?;
        let mut reviewed = Vec::new();
        for _ in 0..rng.random_range(2..=4) {
            guard.check()?;
            guard
                .controller
                .report(app, "VS Code: opening and scrolling another project file");
            let Some(file) = review_editor(e, guard, source, rng, 2, &reviewed)? else {
                if reviewed.is_empty() {
                    guard.controller.report(
                        app,
                        "VS Code: no safely restorable unopened text file is exposed by Explorer",
                    );
                }
                break;
            };
            reviewed.push(file);
        }
        Ok(())
    })();
    let restored = if let Some(session) = explorer {
        desktop::restore_editor_review(
            source.id,
            source.process_id,
            session,
            !guard.controller.session_valid(guard.epoch),
        )
        .map_err(StepError::RestorationFailed)
    } else {
        Ok(())
    };
    restored.and(result)
}

fn review_browser(
    e: &mut Enigo,
    guard: &Guard<'_>,
    source: &desktop::WindowInfo,
    snapshot: &desktop::Snapshot,
    rng: &mut impl Rng,
) -> StepResult {
    let kind = SourceKind::detect(snapshot.domain.as_deref(), &source.application);
    if !kind.can_browse() {
        return Ok(());
    }
    let Ok(browser) = desktop::browser_context(source.id, source.process_id) else {
        return Ok(());
    };
    let Some(address) = SourceKind::browser_address(&browser.address) else {
        return Ok(());
    };
    if SourceKind::detect(address.host_str(), &source.application) != kind
        || !kind.allows_page(&address)
    {
        return Ok(());
    }
    let links: Vec<_> = snapshot
        .links
        .iter()
        .filter_map(|link| Some((SourceKind::browser_address(&link.address)?, link.visited)))
        .filter(|(link, visited)| kind.allows_link(&address, link, *visited))
        .map(|(link, _)| link)
        .collect();
    let address = if links.is_empty() || rng.random_bool(0.4) {
        address
    } else {
        links[rng.random_range(0..links.len())].clone()
    };
    guard.check()?;
    let session = desktop::open_browser_tab(source.id, source.process_id)
        .map_err(StepError::RestorationFailed)?;
    let result = (|| {
        guard.wait(Duration::from_millis(150))?;
        let focused =
            desktop::browser_context(source.id, source.process_id).map_err(StepError::Failed)?;
        if focused.selected != session.created || !focused.address_focused {
            return Err(StepError::Interrupted);
        }
        for character in address.as_str().chars() {
            guard.input(|| e.text(&character.to_string()))?;
            guard.wait(Duration::from_millis(rng.random_range(18..=55)))?;
        }
        guard.input(|| e.key(Key::Return, Direction::Click))?;
        guard.wait(Duration::from_millis(rng.random_range(3_000..=6_000)))?;
        if let Ok(snapshot) = desktop::inspect_window(source.id, source.process_id) {
            if SourceKind::detect(snapshot.domain.as_deref(), &source.application) == kind {
                for _ in 0..rng.random_range(2..=4) {
                    external_action(Action::Move, e, guard, source, Some(&snapshot), rng)?;
                    guard.wait(Duration::from_millis(rng.random_range(1_000..=3_000)))?;
                }
            }
        }
        Ok(())
    })();
    let restored = desktop::close_browser_tab(source.id, source.process_id, session)
        .map_err(StepError::RestorationFailed);
    restored.and(result)
}

pub fn spawn_worker(controller: Arc<Controller>, app: AppHandle) {
    thread::spawn(move || {
        let mut rng = rand::rng();
        let mut enigo = None;
        let mut window_rotation = Vec::new();
        let mut tab_rotations: HashMap<u64, Vec<Vec<i32>>> = HashMap::new();
        loop {
            controller.refresh_tray(&app);
            if !controller.running.load(Ordering::SeqCst) {
                enigo = None;
                window_rotation.clear();
                tab_rotations.clear();
                thread::sleep(Duration::from_millis(120));
                continue;
            }
            if controller.main_visible.load(Ordering::SeqCst) {
                controller.report(&app, "Paused");
                thread::sleep(Duration::from_millis(120));
                continue;
            }
            if desktop::pointer_button_down() {
                controller.user_input();
            }
            if controller.last_input.lock().unwrap().elapsed() < USER_PAUSE {
                controller.report(
                    &app,
                    "Paused for your input; resumes after 8 seconds of inactivity.",
                );
                thread::sleep(Duration::from_millis(60));
                continue;
            }
            let epoch = controller.input_epoch.load(Ordering::SeqCst);
            let result = (|| -> StepResult {
                if enigo.is_none() {
                    enigo = Some(
                        Enigo::new(&Settings {
                            windows_dw_extra_info: Some(desktop::INPUT_MARKER),
                            event_source_user_data: Some(desktop::INPUT_MARKER as i64),
                            ..Settings::default()
                        })
                        .map_err(|error| {
                            controller.fail_current(&app, epoch, &error.to_string());
                            StepError::Interrupted
                        })?,
                    );
                }
                let foreground = desktop::foreground_window();
                let windows = desktop::list_windows().map_err(StepError::Failed)?;
                let discovered = automatic_targets(&windows);
                let mut targets = Vec::new();
                for source in windows
                    .into_iter()
                    .filter(|source| discovered.contains(&source.id))
                {
                    if SourceKind::is_browser(&source.application) {
                        let tabs = desktop::browser_tabs(source.id, source.process_id)
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|tab| SourceKind::allows_reading(&tab.address))
                            .collect::<Vec<_>>();
                        if !tabs.is_empty() || foreground == Some(source.id) {
                            targets.push((source, tabs));
                        }
                    } else if SourceKind::detect(None, &source.application).can_activate()
                        || foreground == Some(source.id)
                    {
                        targets.push((source, Vec::new()));
                    }
                }
                let available: Vec<_> = targets.iter().map(|(source, _)| source.id).collect();
                tab_rotations.retain(|id, _| discovered.contains(id));
                let Some(source_window) = activity_plan::next_target(
                    &mut window_rotation,
                    &available,
                    foreground.as_ref(),
                    &mut rng,
                ) else {
                    controller.report(
                        &app,
                        "Waiting for an available application or readable browser tab...",
                    );
                    thread::sleep(Duration::from_millis(500));
                    return Err(StepError::Interrupted);
                };
                let (source, tabs) = targets
                    .into_iter()
                    .find(|(source, _)| source.id == source_window)
                    .unwrap();
                let guard = Guard {
                    controller: &controller,
                    epoch,
                    window: source_window,
                };
                guard.check_input()?;
                if !tabs.is_empty() {
                    let selected = tabs.iter().find(|tab| tab.selected).map(|tab| &tab.id);
                    let ids: Vec<_> = tabs.iter().map(|tab| tab.id.clone()).collect();
                    let id = activity_plan::next_target(
                        tab_rotations.entry(source.id).or_default(),
                        &ids,
                        selected,
                        &mut rng,
                    )
                    .unwrap();
                    let tab = tabs.into_iter().find(|tab| tab.id == id).unwrap();
                    controller.report(&app, "Browser: selecting an existing tab");
                    desktop::select_browser_tab(
                        source.id,
                        source.process_id,
                        tab.clone(),
                        foreground.unwrap_or(0),
                    )
                    .map_err(StepError::Failed)?;
                    guard.focus()?;
                    let e = enigo.as_mut().unwrap();
                    if let Some(bounds) = desktop::window_bounds(source.id) {
                        smooth_point(
                            e,
                            &guard,
                            (
                                bounds.x + bounds.width * 2 / 3,
                                bounds.y + bounds.height / 2,
                            ),
                            &mut rng,
                        )?;
                    }
                    let page_kind = SourceKind::detect(
                        tauri::Url::parse(&tab.address)
                            .ok()
                            .as_ref()
                            .and_then(tauri::Url::host_str),
                        &source.application,
                    );
                    let select_text = page_kind != SourceKind::Mail && rng.random_bool(0.35);
                    controller.report(
                        &app,
                        if select_text {
                            "Browser: reviewing text and clearing the selection"
                        } else {
                            "Browser: scrolling down and returning to the original position"
                        },
                    );
                    desktop::read_browser_page(
                        source.id,
                        source.process_id,
                        tab.address,
                        select_text,
                        || guard.check().is_err(),
                    )
                    .map_err(|error| {
                        if error.contains("preserving the current view") {
                            StepError::Interrupted
                        } else {
                            StepError::RestorationFailed(error)
                        }
                    })?;
                    guard.check()?;
                    return Ok(());
                }
                if SourceKind::detect(None, &source.application).can_activate() {
                    guard.focus()?;
                }
                let snapshot = desktop::inspect_window(source.id, source.process_id).ok();
                guard.check()?;
                guard.wait(Duration::from_millis(400))?;
                let e = enigo.as_mut().unwrap();
                if SourceKind::detect(None, &source.application) == SourceKind::Ide {
                    controller.report(&app, "IDE: extensions, project tree and a project file");
                    review_editor_sequence(e, &guard, &source, &mut rng, &app)?;
                    return Ok(());
                }
                if let Some(snapshot) = &snapshot {
                    if !SourceKind::is_browser(&source.application) && rng.random_bool(0.3) {
                        review_browser(e, &guard, &source, snapshot, &mut rng)?;
                    }
                }
                let kind = SourceKind::detect(
                    snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.domain.as_deref()),
                    &source.application,
                );
                let can_scroll = kind.can_scroll()
                    && snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.scroll.as_ref())
                        .is_some_and(|scroll| scroll.vertical >= 0.0 || scroll.horizontal >= 0.0);
                let mut plan = Plan::new(can_scroll, &mut rng);
                controller.report(
                    &app,
                    &format!("{}: {}", plan.scenario.label(), kind.label()),
                );
                let started = Instant::now();
                while started.elapsed() < Duration::from_millis(plan.duration_ms) {
                    let (action, pause) = plan.next(&mut rng);
                    external_action(action, e, &guard, &source, snapshot.as_ref(), &mut rng)?;
                    let remaining =
                        Duration::from_millis(plan.duration_ms).saturating_sub(started.elapsed());
                    guard.wait(Duration::from_millis(pause).min(remaining))?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {}
                Err(StepError::Interrupted) => {}
                Err(StepError::Failed(error)) => {
                    controller.report(&app, &format!("Skipping unavailable action: {error}"));
                    thread::sleep(Duration::from_millis(500));
                }
                Err(StepError::RestorationFailed(error)) => {
                    controller.stop(&app);
                    controller.report(
                        &app,
                        &format!("Could not restore the original view: {error}"),
                    );
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn tray_distinguishes_running_input_pause_manual_stop_and_schedule_finish() {
        use crate::tray_status::Status;
        let controller = Controller::new();
        assert_eq!(controller.tray_status(), Status::Idle);
        controller.running.store(true, Ordering::SeqCst);
        assert_eq!(controller.tray_status(), Status::Active);
        controller.user_input();
        assert_eq!(controller.tray_status(), Status::Paused);
        *controller.last_input.lock().unwrap() = Instant::now() - USER_PAUSE;
        assert_eq!(controller.tray_status(), Status::Active);
        controller.main_visible.store(true, Ordering::SeqCst);
        assert_eq!(controller.tray_status(), Status::Paused);
        controller.running.store(false, Ordering::SeqCst);
        controller.stopped_by_user.store(true, Ordering::SeqCst);
        assert_eq!(controller.tray_status(), Status::Paused);
        controller.stopped_by_user.store(false, Ordering::SeqCst);
        assert_eq!(controller.tray_status(), Status::Idle);
    }

    #[test]
    fn all_external_windows_are_discovered_without_saved_permissions() {
        let window = |id, process_id| desktop::WindowInfo {
            id,
            process_id,
            title: format!("Window {id}"),
            application: "editor.exe".into(),
        };
        let mut windows = vec![
            window(1, u32::MAX),
            window(2, u32::MAX),
            window(3, std::process::id()),
        ];
        assert_eq!(automatic_targets(&windows), vec![1, 2]);
        windows.push(window(4, u32::MAX));
        assert!(automatic_targets(&windows).contains(&4));
        windows.remove(0);
        assert!(!automatic_targets(&windows).contains(&1));
    }

    #[test]
    fn unchanged_schedule_preserves_pending_start_and_intent() {
        let controller = Controller::new();
        let shift = Schedule {
            start: "09:00".into(),
            finish: "17:00".into(),
            ..Schedule::default()
        };
        controller.update_schedule(shift.clone());
        let clock = LocalClock {
            day: 1,
            minute: 600,
        };
        let (intent, decision) = controller.schedule_decision(clock);
        assert_eq!(decision, Some(Decision::Start));
        controller.update_schedule(shift);
        assert_eq!(controller.intent_epoch.load(Ordering::SeqCst), intent);
        assert_eq!(controller.schedule_decision(clock).1, None);
        controller.update_schedule(Schedule::default());
        assert_ne!(controller.intent_epoch.load(Ordering::SeqCst), intent);
        assert_eq!(controller.schedule_decision(clock).1, None);
    }

    #[test]
    fn real_input_interrupts_the_current_action_and_its_wait() {
        let controller = Controller::new();
        controller.running.store(true, Ordering::SeqCst);
        let guard = Guard {
            controller: &controller,
            epoch: 0,
            window: 0,
        };
        assert!(guard.check_input().is_ok());
        controller.user_input();
        assert!(matches!(guard.check_input(), Err(StepError::Interrupted)));
        assert!(matches!(
            guard.wait(Duration::from_millis(50)),
            Err(StepError::Interrupted)
        ));
    }

    #[test]
    fn stop_prevents_focusing_or_issuing_more_input() {
        let controller = Controller::new();
        let guard = Guard {
            controller: &controller,
            epoch: 0,
            window: 0,
        };
        assert!(matches!(guard.focus(), Err(StepError::Interrupted)));
        let mut issued = false;
        assert!(matches!(
            guard.input(|| {
                issued = true;
                Ok(())
            }),
            Err(StepError::Interrupted)
        ));
        assert!(!issued);
    }

    #[test]
    fn opening_main_pauses_actions_and_invalidates_pending_input() {
        let controller = Controller::new();
        controller.running.store(true, Ordering::SeqCst);
        assert!(controller.session_valid(0));
        controller.main_visible.store(true, Ordering::SeqCst);
        assert!(!controller.session_valid(0));
        let guard = Guard {
            controller: &controller,
            epoch: 0,
            window: 0,
        };
        assert!(matches!(guard.check_input(), Err(StepError::Interrupted)));
        controller.main_visible.store(false, Ordering::SeqCst);
        controller.user_input();
        assert!(!controller.session_valid(0));
    }

    #[test]
    fn target_rotation_visits_all_windows_without_repeats_or_starvation() {
        let mut rng = StdRng::seed_from_u64(2);
        let targets = [1, 2, 3];
        let mut remaining = Vec::new();
        let mut current = None;
        for _ in 0..100 {
            let mut visited = std::collections::HashSet::new();
            for _ in 0..targets.len() {
                let next = activity_plan::next_target(
                    &mut remaining,
                    &targets,
                    current.as_ref(),
                    &mut rng,
                )
                .unwrap();
                assert_ne!(Some(next), current);
                assert!(visited.insert(next));
                current = Some(next);
            }
            assert_eq!(visited.len(), targets.len());
        }
        assert_eq!(
            activity_plan::next_target(&mut remaining, &[1], Some(&1), &mut rng),
            Some(1)
        );
        assert_eq!(
            activity_plan::next_target(&mut remaining, &[], None, &mut rng),
            None
        );
    }
}
