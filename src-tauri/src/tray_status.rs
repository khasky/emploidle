use std::sync::Mutex;

use tauri::{image::Image, AppHandle, Manager};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Active,
    Paused,
}

pub struct Icons {
    idle: Image<'static>,
    active: Image<'static>,
    paused: Image<'static>,
    current: Mutex<Status>,
}

impl Icons {
    pub fn new() -> tauri::Result<Self> {
        let idle = Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
        Ok(Self {
            active: tint(&idle, [33, 168, 83]),
            paused: tint(&idle, [234, 139, 24]),
            idle,
            current: Mutex::new(Status::Idle),
        })
    }

    pub fn idle(&self) -> Image<'static> {
        self.idle.clone()
    }
}

fn tint(source: &Image<'_>, color: [u8; 3]) -> Image<'static> {
    let mut pixels = source.rgba().to_vec();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        if pixel[2] > pixel[0] && pixel[2] > pixel[1] {
            let intensity = f64::from(pixel[2]) / 255.0;
            for channel in 0..3 {
                pixel[channel] = (f64::from(color[channel]) * intensity).round() as u8;
            }
        }
    }
    Image::new_owned(pixels, source.width(), source.height())
}

pub fn update(app: &AppHandle, status: Status) -> tauri::Result<()> {
    let Some(icons) = app.try_state::<Icons>() else {
        return Ok(());
    };
    if *icons.current.lock().unwrap() == status {
        return Ok(());
    }
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let icons = handle.state::<Icons>();
        let controller = &handle.state::<crate::AppState>().controller;
        // Resolve queued transitions on the UI thread; never hold an icon lock while waiting for it.
        let status = controller.tray_status();
        let mut current = icons.current.lock().unwrap();
        if *current == status {
            return;
        }
        if let Some(tray) = handle.tray_by_id("main") {
            let icon = match status {
                Status::Idle => &icons.idle,
                Status::Active => &icons.active,
                Status::Paused => &icons.paused,
            };
            match tray.set_icon(Some(icon.clone())) {
                Ok(()) => *current = status,
                Err(error) => {
                    drop(current);
                    controller.report(&handle, &format!("Cannot update tray icon: {error}"));
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_icons_keep_shape_and_white_details_with_distinct_colors() {
        let icons = Icons::new().unwrap();
        for icon in [&icons.active, &icons.paused] {
            assert_eq!(icon.width(), icons.idle.width());
            assert_eq!(icon.height(), icons.idle.height());
            assert_eq!(icon.rgba().len(), icons.idle.rgba().len());
            for (original, recolored) in icons
                .idle
                .rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .zip(icon.rgba().as_chunks::<4>().0.iter())
            {
                assert_eq!(original[3], recolored[3]);
                if original[..3] == [255, 255, 255] {
                    assert_eq!(original, recolored);
                }
            }
        }
        assert!(icons
            .active
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| u16::from(p[1]) > 2 * u16::from(p[0]) && p[1] > p[2]));
        assert!(icons
            .paused
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] > p[1] && p[1] > p[2]));
    }
}
