use futures::channel::mpsc::UnboundedSender;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Active,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Show,
    Start,
    Stop,
    Quit,
}

const MENU: [(Command, &str); 4] = [
    (Command::Show, "Show window"),
    (Command::Start, "Start"),
    (Command::Stop, "Stop"),
    (Command::Quit, "Quit"),
];

#[derive(Clone)]
pub struct Image {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

impl Image {
    fn from_png(bytes: &[u8]) -> Result<Self, String> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
        let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
        let mut pixels = vec![
            0;
            reader
                .output_buffer_size()
                .ok_or("Tray icon is too large")?
        ];
        let info = reader
            .next_frame(&mut pixels)
            .map_err(|error| error.to_string())?;
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return Err("Tray icon must be an 8-bit RGBA PNG".into());
        }
        pixels.truncate(info.buffer_size());
        Ok(Self {
            rgba: pixels,
            width: info.width,
            height: info.height,
        })
    }
}

pub struct Icons {
    idle: Image,
    active: Image,
    paused: Image,
}

impl Icons {
    pub fn new() -> Result<Self, String> {
        let idle = Image::from_png(include_bytes!("../icons/32x32.png"))?;
        Ok(Self {
            active: tint(&idle, [33, 168, 83]),
            paused: tint(&idle, [234, 139, 24]),
            idle,
        })
    }

    fn get(&self, status: Status) -> &Image {
        match status {
            Status::Idle => &self.idle,
            Status::Active => &self.active,
            Status::Paused => &self.paused,
        }
    }
}

fn tint(source: &Image, color: [u8; 3]) -> Image {
    let mut pixels = source.rgba.clone();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        if pixel[2] > pixel[0] && pixel[2] > pixel[1] {
            let intensity = f64::from(pixel[2]) / 255.0;
            for channel in 0..3 {
                pixel[channel] = (f64::from(color[channel]) * intensity).round() as u8;
            }
        }
    }
    Image {
        rgba: pixels,
        width: source.width,
        height: source.height,
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::*;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

    pub struct Tray {
        tray: TrayIcon,
        icons: Icons,
    }

    fn icon(image: &Image) -> Result<Icon, String> {
        Icon::from_rgba(image.rgba.clone(), image.width, image.height)
            .map_err(|error| error.to_string())
    }

    impl Tray {
        pub fn new(
            tooltip: &str,
            icons: Icons,
            commands: UnboundedSender<Command>,
        ) -> Result<Self, String> {
            let menu = Menu::new();
            let mut ids = Vec::new();
            for (command, label) in MENU {
                let item = MenuItem::new(label, true, None);
                ids.push((item.id().clone(), command));
                menu.append(&item).map_err(|error| error.to_string())?;
            }
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if let Some((_, command)) = ids.iter().find(|(id, _)| *id == event.id) {
                    let _ = commands.unbounded_send(*command);
                }
            }));
            let tray = TrayIconBuilder::new()
                .with_icon(icon(icons.get(Status::Idle))?)
                .with_tooltip(tooltip)
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(true)
                .build()
                .map_err(|error| error.to_string())?;
            Ok(Self { tray, icons })
        }

        pub fn set_status(&mut self, status: Status) -> Result<(), String> {
            self.tray
                .set_icon(Some(icon(self.icons.get(status))?))
                .map_err(|error| error.to_string())
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use ksni::blocking::{Handle, TrayMethods};

    struct Item {
        tooltip: String,
        icon: ksni::Icon,
        commands: UnboundedSender<Command>,
    }

    fn icon(image: &Image) -> ksni::Icon {
        let mut data = image.rgba.clone();
        for pixel in data.as_chunks_mut::<4>().0 {
            pixel.rotate_right(1);
        }
        ksni::Icon {
            width: image.width as i32,
            height: image.height as i32,
            data,
        }
    }

    impl ksni::Tray for Item {
        const MENU_ON_ACTIVATE: bool = true;

        fn id(&self) -> String {
            "emploidle".into()
        }

        fn title(&self) -> String {
            self.tooltip.clone()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            vec![self.icon.clone()]
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: self.tooltip.clone(),
                ..Default::default()
            }
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            MENU.iter()
                .map(|&(command, label)| {
                    ksni::menu::StandardItem {
                        label: label.into(),
                        activate: Box::new(move |this: &mut Self| {
                            let _ = this.commands.unbounded_send(command);
                        }),
                        ..Default::default()
                    }
                    .into()
                })
                .collect()
        }
    }

    pub struct Tray {
        handle: Handle<Item>,
        icons: Icons,
    }

    impl Tray {
        pub fn new(
            tooltip: &str,
            icons: Icons,
            commands: UnboundedSender<Command>,
        ) -> Result<Self, String> {
            let handle = Item {
                tooltip: tooltip.into(),
                icon: icon(icons.get(Status::Idle)),
                commands,
            }
            .spawn()
            .map_err(|error| error.to_string())?;
            Ok(Self { handle, icons })
        }

        pub fn set_status(&mut self, status: Status) -> Result<(), String> {
            let icon = icon(self.icons.get(status));
            self.handle
                .update(|item| item.icon = icon)
                .ok_or_else(|| "The tray service has stopped".to_string())
        }
    }
}

pub use platform::Tray;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_icons_keep_shape_and_white_details_with_distinct_colors() {
        let icons = Icons::new().unwrap();
        for icon in [&icons.active, &icons.paused] {
            assert_eq!(icon.width, icons.idle.width);
            assert_eq!(icon.height, icons.idle.height);
            assert_eq!(icon.rgba.len(), icons.idle.rgba.len());
            for (original, recolored) in icons
                .idle
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .zip(icon.rgba.as_chunks::<4>().0.iter())
            {
                assert_eq!(original[3], recolored[3]);
                if original[..3] == [255, 255, 255] {
                    assert_eq!(original, recolored);
                }
            }
        }
        assert!(icons
            .active
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| u16::from(p[1]) > 2 * u16::from(p[0]) && p[1] > p[2]));
        assert!(icons
            .paused
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] > p[1] && p[1] > p[2]));
    }
}
