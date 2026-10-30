//! Background worker that imitates human "working" activity: paced scrolling
//! ("reading"), switching between applications and tabs, and small mouse
//! movements. Driven by a shared flag toggled from the UI / tray.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use enigo::{Axis, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use rand::Rng;

/// Modifier used to switch between applications: Cmd on macOS, Alt elsewhere.
fn app_switch_mod() -> Key {
    #[cfg(target_os = "macos")]
    {
        Key::Meta
    }
    #[cfg(not(target_os = "macos"))]
    {
        Key::Alt
    }
}

/// Sleep that wakes up early when the running flag is cleared, so Stop feels
/// immediate instead of waiting out a long pause.
fn nap(ms: u64, running: &AtomicBool) {
    let mut left = ms;
    while left > 0 && running.load(Ordering::Relaxed) {
        let step = left.min(120);
        thread::sleep(Duration::from_millis(step));
        left -= step;
    }
}

fn active(running: &AtomicBool) -> bool {
    running.load(Ordering::Relaxed)
}

/// Scroll down in small steps with reading-paced pauses, then jump back up —
/// as a person skimming a page would.
fn read_scroll(e: &mut Enigo, rng: &mut impl Rng, running: &AtomicBool) {
    let steps = rng.random_range(6..=18);
    for _ in 0..steps {
        if !active(running) {
            return;
        }
        let _ = e.scroll(1, Axis::Vertical);
        if rng.random_bool(0.25) {
            let _ = e.move_mouse(rng.random_range(-6..=6), rng.random_range(-4..=4), Coordinate::Rel);
        }
        nap(rng.random_range(280..=950), running);
        if rng.random_bool(0.12) {
            nap(rng.random_range(800..=2200), running); // longer "reading" pause
        }
    }
    // back to the top in a few quicker scrolls
    let up = rng.random_range(4..=8);
    for _ in 0..up {
        if !active(running) {
            return;
        }
        let _ = e.scroll(-3, Axis::Vertical);
        nap(rng.random_range(60..=160), running);
    }
}

/// Alt/Cmd+Tab to bring another window to the front.
fn switch_app(e: &mut Enigo, rng: &mut impl Rng, running: &AtomicBool) {
    let m = app_switch_mod();
    let _ = e.key(m, Direction::Press);
    for _ in 0..rng.random_range(1..=2) {
        let _ = e.key(Key::Tab, Direction::Click);
        nap(rng.random_range(140..=320), running);
    }
    let _ = e.key(m, Direction::Release);
    nap(rng.random_range(400..=900), running);
}

/// Ctrl+Tab through a few tabs of the focused browser / editor.
fn switch_tab(e: &mut Enigo, rng: &mut impl Rng, running: &AtomicBool) {
    let _ = e.key(Key::Control, Direction::Press);
    for _ in 0..rng.random_range(1..=3) {
        let _ = e.key(Key::Tab, Direction::Click);
        nap(rng.random_range(200..=500), running);
    }
    let _ = e.key(Key::Control, Direction::Release);
}

/// Small idle mouse movements.
fn micro_move(e: &mut Enigo, rng: &mut impl Rng, running: &AtomicBool) {
    for _ in 0..rng.random_range(2..=5) {
        if !active(running) {
            return;
        }
        let _ = e.move_mouse(rng.random_range(-25..=25), rng.random_range(-18..=18), Coordinate::Rel);
        nap(rng.random_range(120..=360), running);
    }
}

fn simulate_tick(e: &mut Enigo, rng: &mut impl Rng, running: &AtomicBool) {
    match rng.random_range(0..100) {
        0..=49 => read_scroll(e, rng, running),
        50..=69 => micro_move(e, rng, running),
        70..=86 => switch_tab(e, rng, running),
        _ => switch_app(e, rng, running),
    }
}

/// Spawn the activity loop. It idles cheaply while the flag is off and creates
/// the input backend lazily on first Start (so macOS asks for the Accessibility
/// permission only when activity is actually requested).
pub fn spawn_worker(running: Arc<AtomicBool>) {
    thread::spawn(move || {
        let mut enigo: Option<Enigo> = None;
        let mut rng = rand::rng();
        loop {
            if !active(&running) {
                thread::sleep(Duration::from_millis(200));
                continue;
            }
            if enigo.is_none() {
                match Enigo::new(&Settings::default()) {
                    Ok(e) => enigo = Some(e),
                    Err(err) => {
                        eprintln!("Emploidle: input backend unavailable: {err}");
                        thread::sleep(Duration::from_secs(2));
                        continue;
                    }
                }
            }
            let e = enigo.as_mut().unwrap();
            simulate_tick(e, &mut rng, &running);
            nap(rng.random_range(1500..=5200), &running);
        }
    });
}
