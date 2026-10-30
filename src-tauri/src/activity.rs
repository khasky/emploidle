use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use rand::Rng;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::activity_plan::{self, Action, Context, Plan};
use crate::desktop::{self, Bounds};
use crate::schedule::{Decision, LocalClock, Schedule, Scheduler};
use crate::source_context::SourceKind;

pub const PRACTICE_TITLE: &str = "Emploidle - Practice document";
pub const PREVIEW_TITLE: &str = "Emploidle - Preview";
const USER_PAUSE: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TargetContext {
    Reading,
    Preview,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Target {
    pub id: u64,
    pub context: TargetContext,
    #[serde(skip)]
    pub process_id: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Configuration {
    pub targets: Vec<Target>,
    pub practice: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PreviewData {
    pub title: String,
    pub domain: Option<String>,
    pub kind: String,
    pub text: String,
}

pub struct Controller {
    pub running: AtomicBool,
    pub configuration: Mutex<Configuration>,
    status: Mutex<String>,
    input_epoch: AtomicU64,
    last_input: Mutex<Instant>,
    pub preview: Mutex<Option<PreviewData>>,
    copy_text: Mutex<String>,
    pub practice_sequence: AtomicU64,
    pub practice_completed: AtomicU64,
    pub main_visible: AtomicBool,
    pub scheduler: Mutex<Scheduler>,
    pub intent_epoch: AtomicU64,
    intent_lock: Mutex<()>,
}

impl Controller {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            configuration: Mutex::new(Configuration::default()),
            status: Mutex::new("Idle".into()),
            input_epoch: AtomicU64::new(0),
            last_input: Mutex::new(Instant::now()),
            preview: Mutex::new(None),
            copy_text: Mutex::new(String::new()),
            practice_sequence: AtomicU64::new(0),
            practice_completed: AtomicU64::new(0),
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
        self.intent_epoch.fetch_add(1, Ordering::SeqCst);
        if let Ok(clock) = LocalClock::now() {
            self.scheduler.lock().unwrap().manual_stop(clock);
        }
        self.stop_activity(app);
    }

    pub fn stop_for_schedule(&self, app: &AppHandle, expected_intent: u64) {
        let _intent = self.intent_lock.lock().unwrap();
        if self.intent_epoch.load(Ordering::SeqCst) == expected_intent {
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
        if let Some(window) = app.get_webview_window("practice") {
            let _ = window.emit("practice-cancel", ());
        }
    }

    fn user_input(&self) {
        *self.last_input.lock().unwrap() = Instant::now();
        self.input_epoch.fetch_add(1, Ordering::SeqCst);
    }

    pub fn set_main_visible(&self, app: &AppHandle, visible: bool) {
        self.main_visible.store(visible, Ordering::SeqCst);
        self.input_epoch.fetch_add(1, Ordering::SeqCst);
        if visible {
            if let Some(window) = app.get_webview_window("practice") {
                let _ = window.emit("practice-cancel", ());
            }
        }
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

    pub fn update_configuration(
        &self,
        configuration: Configuration,
        save: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let _intent = self.intent_lock.lock().unwrap();
        if self.running.load(Ordering::SeqCst) {
            return Err("Stop activity before changing allowed windows.".into());
        }
        save()?;
        self.intent_epoch.fetch_add(1, Ordering::SeqCst);
        self.scheduler.lock().unwrap().cancel_pending_start();
        *self.configuration.lock().unwrap() = configuration;
        Ok(())
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

    pub fn can_type(&self, id: u64, epoch: u64) -> bool {
        self.session_valid(epoch) && self.practice_sequence.load(Ordering::SeqCst) == id
    }

    fn session_valid(&self, epoch: u64) -> bool {
        self.running.load(Ordering::SeqCst)
            && !self.main_visible.load(Ordering::SeqCst)
            && self.input_epoch.load(Ordering::SeqCst) == epoch
    }

    fn remember_text(&self, text: &str) {
        *self.copy_text.lock().unwrap() = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(160)
            .collect();
    }

    pub fn open_preview(
        &self,
        app: &AppHandle,
        source_id: u64,
        epoch: Option<u64>,
    ) -> Result<u64, String> {
        let source = desktop::list_windows()?
            .into_iter()
            .find(|window| window.id == source_id && window.process_id != std::process::id())
            .ok_or("The source window is no longer available.")?;
        if epoch.is_some()
            && !self
                .configuration
                .lock()
                .unwrap()
                .targets
                .iter()
                .any(|target| target.id == source.id && target.process_id == source.process_id)
        {
            return Err("The source window no longer belongs to the allowed application.".into());
        }
        let snapshot = desktop::inspect_window(source.id, source.process_id)?;
        if epoch.is_some_and(|epoch| !self.session_valid(epoch)) {
            return Err("The preview was cancelled by user input.".into());
        }
        if snapshot.text.trim().is_empty() {
            return Err("This application does not expose readable document text. Hovering is still available.".into());
        }
        let kind = SourceKind::detect(snapshot.domain.as_deref(), &source.application);
        let data = PreviewData {
            title: source.title,
            domain: snapshot.domain,
            kind: kind.label().into(),
            text: snapshot.text,
        };
        self.remember_text(&data.text);
        *self.preview.lock().unwrap() = Some(data.clone());
        let window = if let Some(window) = app.get_webview_window("preview") {
            window.show().map_err(|error| error.to_string())?;
            if epoch.is_none() {
                window.set_focus().map_err(|error| error.to_string())?;
            }
            window
        } else {
            WebviewWindowBuilder::new(app, "preview", WebviewUrl::App("preview.html".into()))
                .title(PREVIEW_TITLE)
                .focused(epoch.is_none())
                .inner_size(760.0, 600.0)
                .min_inner_size(480.0, 360.0)
                .build()
                .map_err(|error| error.to_string())?
        };
        let _ = window.emit("preview-changed", &data);
        desktop::list_windows()?
            .into_iter()
            .find(|window| window.process_id == std::process::id() && window.title == PREVIEW_TITLE)
            .map(|window| window.id)
            .ok_or("The local preview window could not be found.".into())
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
        configuration: Configuration,
    ) -> Result<(), String> {
        let _intent = self.intent_lock.lock().unwrap();
        if self.intent_epoch.load(Ordering::SeqCst) != expected_intent {
            return Ok(());
        }
        *self.configuration.lock().unwrap() = configuration;
        self.activate(app)
    }

    fn activate(self: &Arc<Self>, app: &AppHandle) -> Result<(), String> {
        let configuration = self.configuration.lock().unwrap().clone();
        let targets = available_targets(&configuration)?;
        if targets.is_empty() {
            return Err("Choose an allowed window or enable the practice document.".into());
        }
        let input_controller = self.clone();
        let stop_controller = self.clone();
        let stop_app = app.clone();
        desktop::start_monitor(
            Arc::new(move || input_controller.user_input()),
            Arc::new(move || stop_controller.stop(&stop_app)),
        )?;
        self.user_input();
        self.running.store(true, Ordering::SeqCst);
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

fn available_targets(configuration: &Configuration) -> Result<Vec<(u64, Context)>, String> {
    let windows = desktop::list_windows()?;
    let mut targets = Vec::new();
    for target in &configuration.targets {
        if windows.iter().any(|window| {
            window.id == target.id
                && window.process_id == target.process_id
                && window.process_id != std::process::id()
        }) {
            targets.push((
                target.id,
                match target.context {
                    TargetContext::Reading => Context::Reading,
                    TargetContext::Preview => Context::Preview,
                },
            ));
        }
    }
    if configuration.practice {
        if let Some(window) = windows.iter().find(|window| {
            window.process_id == std::process::id() && window.title == PRACTICE_TITLE
        }) {
            targets.push((window.id, Context::Practice));
        }
    }
    Ok(targets)
}

#[derive(Debug)]
enum StepError {
    Interrupted,
    Failed(String),
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
        if desktop::foreground_window() != Some(self.window) {
            return Err(StepError::Failed(
                "The allowed window lost focus. Start again when ready.".into(),
            ));
        }
        Ok(())
    }

    fn focus(&self) -> StepResult {
        self.check_input()?;
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
                return Err(StepError::Failed(
                    "The selected window could not be focused.".into(),
                ));
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
}

fn chord(e: &mut Enigo, guard: &Guard<'_>, modifiers: &[Key], key: Key) -> StepResult {
    let mut held = Vec::new();
    let result = (|| {
        for modifier in modifiers {
            guard.input(|| e.key(*modifier, Direction::Press))?;
            held.push(*modifier);
        }
        guard.input(|| e.key(key, Direction::Click))
    })();
    let mut release_error = None;
    for modifier in held.iter().rev() {
        if let Err(error) = e.key(*modifier, Direction::Release) {
            release_error = Some(StepError::Failed(error.to_string()));
        }
    }
    result.and(release_error.map_or(Ok(()), Err))
}

fn smooth_move(
    e: &mut Enigo,
    guard: &Guard<'_>,
    bounds: Bounds,
    rng: &mut impl Rng,
    document: bool,
) -> StepResult {
    let margin = 24;
    if bounds.width < 120 || bounds.height < if document { 220 } else { 120 } {
        return Err(StepError::Failed(
            "The selected window is too small. Enlarge it and start again.".into(),
        ));
    }
    let x = rng.random_range(bounds.x + margin..bounds.x + bounds.width - margin);
    let top = if document { 120 } else { 60 };
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
    let steps = rng.random_range(24..=48);
    let path = activity_plan::pointer_path(start, end, rng.random_range(-0.18..=0.18), steps);
    for point in path {
        guard.input(|| e.move_mouse(point.0, point.1, Coordinate::Abs))?;
        guard.wait(Duration::from_millis(rng.random_range(8..=16)))?;
    }
    if rng.random_bool(0.25) {
        guard.wait(Duration::from_millis(rng.random_range(100..=250)))?;
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

fn ensure_pointer(
    e: &mut Enigo,
    guard: &Guard<'_>,
    bounds: Bounds,
    rng: &mut impl Rng,
    practice: bool,
) -> StepResult {
    let (x, y) = e
        .location()
        .map_err(|error| StepError::Failed(error.to_string()))?;
    if x < bounds.x + 24
        || x >= bounds.x + bounds.width - 24
        || y < bounds.y + if practice { 120 } else { 60 }
        || y >= bounds.y + bounds.height - 24
    {
        smooth_move(e, guard, bounds, rng, practice)?;
    }
    Ok(())
}

fn execute(
    action: Action,
    e: &mut Enigo,
    guard: &Guard<'_>,
    context: Context,
    rng: &mut impl Rng,
    sample: &str,
    app: &AppHandle,
) -> StepResult {
    guard.check()?;
    if activity_plan::is_editing(action) {
        let windows = desktop::list_windows().map_err(StepError::Failed)?;
        if context != Context::Practice
            || !windows.iter().any(|window| {
                window.id == guard.window
                    && window.process_id == std::process::id()
                    && window.title == PRACTICE_TITLE
            })
        {
            return Err(StepError::Failed(
                "Editing is restricted to the practice document.".into(),
            ));
        }
    }
    let bounds = desktop::window_bounds(guard.window)
        .ok_or_else(|| StepError::Failed("The allowed window is no longer available.".into()))?;
    match action {
        Action::Pause => Ok(()),
        Action::Move => smooth_move(e, guard, bounds, rng, context == Context::Practice),
        Action::Scroll(amount) | Action::HorizontalScroll(amount) => {
            if context == Context::Reading {
                return Err(StepError::Failed(
                    "Scrolling is restricted to local documents.".into(),
                ));
            }
            ensure_pointer(e, guard, bounds, rng, context == Context::Practice)?;
            let axis = if matches!(action, Action::HorizontalScroll(_)) {
                Axis::Horizontal
            } else {
                Axis::Vertical
            };
            for _ in 0..amount.unsigned_abs() {
                guard.input(|| e.scroll(amount.signum(), axis))?;
                guard.wait(Duration::from_millis(rng.random_range(90..=280)))?;
            }
            Ok(())
        }
        Action::Page { up } => {
            if context != Context::Preview {
                return Err(StepError::Failed(
                    "Page keys are restricted to the local preview.".into(),
                ));
            }
            guard.input(|| {
                e.key(
                    if up { Key::PageUp } else { Key::PageDown },
                    Direction::Click,
                )
            })
        }
        Action::Caret { back } => {
            let key = if rng.random_bool(0.2) {
                if back {
                    Key::UpArrow
                } else {
                    Key::DownArrow
                }
            } else if back {
                Key::LeftArrow
            } else {
                Key::RightArrow
            };
            for _ in 0..rng.random_range(1..=5) {
                guard.input(|| e.key(key, Direction::Click))?;
                guard.wait(Duration::from_millis(rng.random_range(90..=210)))?;
            }
            Ok(())
        }
        Action::SelectText => {
            for _ in 0..rng.random_range(3..=9) {
                chord(e, guard, &[Key::Shift], Key::LeftArrow)?;
                guard.wait(Duration::from_millis(rng.random_range(80..=160)))?;
            }
            guard.wait(Duration::from_millis(rng.random_range(500..=1500)))?;
            guard.input(|| e.key(Key::RightArrow, Direction::Click))
        }
        Action::ClickDocument => {
            smooth_move(e, guard, bounds, rng, true)?;
            guard.input(|| e.button(Button::Left, Direction::Click))
        }
        Action::Type => {
            const SENTENCES: [&str; 5] = [
                "Review the notes and outline the next steps. ",
                "Compare the examples before continuing. ",
                "Keep the draft concise and check each section. ",
                "Summarize the findings in a short paragraph. ",
                "Return to this detail after reading the document. ",
            ];
            let text = if sample.is_empty() {
                SENTENCES[rng.random_range(0..SENTENCES.len())]
            } else {
                sample
            };
            guard.check()?;
            let id = guard
                .controller
                .practice_sequence
                .fetch_add(1, Ordering::SeqCst)
                + 1;
            let window = app
                .get_webview_window("practice")
                .ok_or_else(|| StepError::Failed("The practice document is closed.".into()))?;
            window
                .emit(
                    "practice-type",
                    serde_json::json!({"id": id, "epoch": guard.epoch, "text": text}),
                )
                .map_err(|error| StepError::Failed(error.to_string()))?;
            let deadline = Instant::now() + Duration::from_secs(90);
            while guard.controller.practice_completed.load(Ordering::SeqCst) != id {
                guard.wait(Duration::from_millis(30))?;
                if Instant::now() >= deadline {
                    return Err(StepError::Failed(
                        "The practice document did not complete its typing action.".into(),
                    ));
                }
            }
            Ok(())
        }
    }
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
    if !matches!(
        action,
        Action::Move | Action::Scroll(_) | Action::HorizontalScroll(_)
    ) {
        return Err(StepError::Failed(
            "Navigation and editing are disabled in external windows.".into(),
        ));
    }
    let bounds = desktop::window_bounds(source.id)
        .ok_or_else(|| StepError::Failed("The selected window is no longer available.".into()))?;
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
        .collect();
    if rows.is_empty() {
        return smooth_move(e, guard, bounds, rng, false);
    }
    let row = rows[rng.random_range(0..rows.len())];
    smooth_point(
        e,
        guard,
        (row.x + row.width / 2, row.y + row.height / 2),
        rng,
    )
}

fn choose_target(
    targets: &[(u64, Context)],
    current: Option<u64>,
    previous: Option<u64>,
    rng: &mut impl Rng,
) -> (u64, Context) {
    if targets.len() > 1 {
        if rng.random_bool(0.65) {
            if let Some(target) = targets
                .iter()
                .find(|target| Some(target.0) == previous && Some(target.0) != current)
            {
                return *target;
            }
        }
        let alternatives: Vec<_> = targets
            .iter()
            .copied()
            .filter(|target| Some(target.0) != current)
            .collect();
        if !alternatives.is_empty() {
            return alternatives[rng.random_range(0..alternatives.len())];
        }
    }
    targets[rng.random_range(0..targets.len())]
}

pub fn spawn_worker(controller: Arc<Controller>, app: AppHandle) {
    thread::spawn(move || {
        let mut rng = rand::rng();
        let mut enigo = None;
        let mut current = None;
        let mut previous = None;
        loop {
            if !controller.running.load(Ordering::SeqCst) {
                enigo = None;
                current = None;
                previous = None;
                thread::sleep(Duration::from_millis(120));
                continue;
            }
            if controller.main_visible.load(Ordering::SeqCst) {
                controller.report(&app, "Paused");
                thread::sleep(Duration::from_millis(120));
                continue;
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
                        .map_err(|error| StepError::Failed(error.to_string()))?,
                    );
                }
                let configuration = controller.configuration.lock().unwrap().clone();
                let targets = available_targets(&configuration).map_err(StepError::Failed)?;
                if targets.is_empty() {
                    return Err(StepError::Failed(
                        "No allowed windows remain. Choose a window and start again.".into(),
                    ));
                }
                let (source_window, context) = choose_target(&targets, current, previous, &mut rng);
                previous = current;
                current = Some(source_window);
                let window = if context == Context::Preview {
                    let preview = controller.open_preview(&app, source_window, Some(epoch));
                    if !controller.session_valid(epoch) {
                        return Err(StepError::Interrupted);
                    }
                    preview.map_err(StepError::Failed)?
                } else {
                    source_window
                };
                if !controller.session_valid(epoch) {
                    return Err(StepError::Interrupted);
                }
                let guard = Guard {
                    controller: &controller,
                    epoch,
                    window,
                };
                let source = if context == Context::Reading {
                    desktop::list_windows()
                        .map_err(StepError::Failed)?
                        .into_iter()
                        .find(|source| {
                            source.id == source_window
                                && configuration.targets.iter().any(|target| {
                                    target.id == source.id && target.process_id == source.process_id
                                })
                        })
                } else {
                    None
                };
                let snapshot = source
                    .as_ref()
                    .and_then(|source| desktop::inspect_window(source.id, source.process_id).ok());
                if let Some(snapshot) = &snapshot {
                    controller.remember_text(&snapshot.text);
                }
                if let Some(source) = &source {
                    guard.check_input()?;
                    if desktop::foreground_window() != Some(source.id) {
                        return Err(StepError::Failed("This window is not active. Choose Local preview to avoid changing message read states.".into()));
                    }
                }
                if context == Context::Reading {
                    guard.check()?;
                } else {
                    guard.focus()?;
                }
                guard.wait(Duration::from_millis(400))?;
                let e = enigo.as_mut().unwrap();
                if context == Context::Practice {
                    execute(
                        Action::ClickDocument,
                        e,
                        &guard,
                        context,
                        &mut rng,
                        "",
                        &app,
                    )?;
                }
                let mut plan = Plan::new(context, &mut rng);
                let started = Instant::now();
                while started.elapsed() < Duration::from_millis(plan.duration_ms) {
                    let label = if context == Context::Reading {
                        "Hovering; external navigation and editing are disabled"
                    } else {
                        plan.scenario.label()
                    };
                    controller.report(&app, label);
                    let (action, pause) = plan.next(&mut rng);
                    let sample = controller.copy_text.lock().unwrap().clone();
                    if context == Context::Reading {
                        let source = source.as_ref().ok_or_else(|| {
                            StepError::Failed("The selected window is no longer available.".into())
                        })?;
                        external_action(action, e, &guard, source, snapshot.as_ref(), &mut rng)?;
                    } else {
                        execute(action, e, &guard, context, &mut rng, &sample, &app)?;
                    }
                    guard.wait(Duration::from_millis(pause))?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {}
                Err(StepError::Interrupted) => {
                    if let Some(window) = app.get_webview_window("practice") {
                        let _ = window.emit("practice-cancel", ());
                    }
                }
                Err(StepError::Failed(error)) => {
                    controller.fail_current(&app, epoch, &error);
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
    fn preference_updates_preserve_intent_and_configuration_changes_retry_pending_start() {
        let controller = Controller::new();
        let shift = Schedule {
            start: "09:00".into(),
            finish: "17:00".into(),
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
        assert!(controller
            .update_configuration(Configuration::default(), || Err("save failed".into()))
            .is_err());
        assert_eq!(controller.intent_epoch.load(Ordering::SeqCst), intent);
        controller
            .update_configuration(Configuration::default(), || Ok(()))
            .unwrap();
        let (_, decision) = controller.schedule_decision(clock);
        assert_eq!(decision, Some(Decision::Start));
        controller.running.store(true, Ordering::SeqCst);
        let mut saved = false;
        assert!(controller
            .update_configuration(Configuration::default(), || {
                saved = true;
                Ok(())
            })
            .is_err());
        assert!(!saved);
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
    fn opening_main_pauses_actions_and_invalidates_pending_practice() {
        let controller = Controller::new();
        controller.running.store(true, Ordering::SeqCst);
        controller.practice_sequence.store(4, Ordering::SeqCst);
        assert!(controller.can_type(4, 0));
        assert!(!controller.can_type(3, 0));
        controller.main_visible.store(true, Ordering::SeqCst);
        assert!(!controller.can_type(4, 0));
        let guard = Guard {
            controller: &controller,
            epoch: 0,
            window: 0,
        };
        assert!(matches!(guard.check_input(), Err(StepError::Interrupted)));
        controller.main_visible.store(false, Ordering::SeqCst);
        controller.user_input();
        assert!(!controller.can_type(4, 0));
    }

    #[test]
    fn target_memory_returns_to_previous_windows_and_stays_in_the_allowlist() {
        let mut rng = StdRng::seed_from_u64(2);
        let targets = [
            (1, Context::Reading),
            (2, Context::Preview),
            (3, Context::Reading),
        ];
        let mut returns = 0;
        for _ in 0..100 {
            let target = choose_target(&targets, Some(2), Some(1), &mut rng);
            assert!(targets.contains(&target));
            assert_ne!(target.0, 2);
            returns += usize::from(target.0 == 1);
        }
        assert!(returns > 50);
        assert_eq!(choose_target(&targets[..1], Some(1), None, &mut rng).0, 1);
    }
}
