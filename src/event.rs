//! Events delivered to the UI thread through a single channel.

use std::io;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread;

use ratatui::crossterm::event;
use ratatui_image::sliced::SlicedProtocol;

use crate::worker::Status;

pub enum Event {
    Input(io::Result<event::Event>),
    Pages(Vec<Arc<SlicedProtocol>>),
    Status(Status),
}

/// Forwards terminal input to `events` from a dedicated thread.
///
/// Must start only after any terminal queries (such as the image `Picker`'s), since it
/// consumes everything arriving on stdin.
pub fn spawn_input(events: Sender<Event>) {
    thread::spawn(move || {
        loop {
            let input = event::read();
            let failed = input.is_err();
            if events.send(Event::Input(input)).is_err() || failed {
                break;
            }
        }
    });
}
