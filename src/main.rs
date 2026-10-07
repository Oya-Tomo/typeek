mod app;
mod event;
mod strip;
mod worker;
mod world;

use std::path::PathBuf;
use std::sync::mpsc;

use color_eyre::eyre::{Result, eyre};
use ratatui_image::picker::Picker;

use crate::app::App;
use crate::world::PreviewWorld;

fn main() -> Result<()> {
    color_eyre::install()?;
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| eyre!("usage: typeek <file.typ>"))?;
    // Fail on a bad path before taking over the terminal.
    let world = PreviewWorld::new(&path)?;
    let title = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    ratatui::run(|terminal| {
        // Query the terminal before the input thread starts consuming stdin.
        let picker = Picker::from_query_stdio()?;
        let (events, inbox) = mpsc::channel();
        event::spawn_input(events.clone());
        let worker = worker::spawn(world, picker.clone(), events);
        App::new(title, picker.protocol_type(), worker, inbox).run(terminal)
    })
}
