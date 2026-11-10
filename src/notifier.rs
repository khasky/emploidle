use std::sync::{Arc, Mutex};

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};

use crate::tray_status::Status;

pub enum UiEvent {
    Status(String),
    Running(bool),
    Tray(Status),
}

/// Delivers state changes from worker threads to the UI thread.
#[derive(Clone)]
pub struct Notifier {
    sender: UnboundedSender<UiEvent>,
    tray: Arc<Mutex<Option<Status>>>,
}

impl Notifier {
    pub fn new() -> (Self, UnboundedReceiver<UiEvent>) {
        let (sender, receiver) = unbounded();
        let notifier = Self {
            sender,
            tray: Arc::new(Mutex::new(None)),
        };
        (notifier, receiver)
    }

    pub fn status(&self, message: &str) {
        let _ = self.sender.unbounded_send(UiEvent::Status(message.into()));
    }

    pub fn running(&self, running: bool) {
        let _ = self.sender.unbounded_send(UiEvent::Running(running));
    }

    pub fn tray(&self, status: Status) {
        let mut current = self.tray.lock().unwrap();
        if *current != Some(status) {
            *current = Some(status);
            let _ = self.sender.unbounded_send(UiEvent::Tray(status));
        }
    }
}
