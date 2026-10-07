//! Background compilation, rasterization, and image encoding.
//!
//! The worker owns the [`PreviewWorld`] and turns each document into one [`SlicedProtocol`]
//! per page. Pages are cached by content hash and width, so an edit only re-encodes the
//! pages it changed, and scrolling never re-encodes anything.

use std::collections::HashMap;
use std::iter;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use image::RgbaImage;
use ratatui_image::errors::Errors;
use ratatui_image::picker::Picker;
use ratatui_image::sliced::SlicedProtocol;
use typst::diag::{StrResult, Warned};
use typst::utils::{Scalar, hash128};
use typst_kit::watcher::Watcher;
use typst_layout::{Page, PagedDocument};
use typst_render::RenderOptions;

use crate::event::Event;
use crate::world::PreviewWorld;

pub enum Request {
    /// A dependency of the document changed on disk.
    Recompile,
    /// Pages should now be rendered this many cells wide.
    Resize(u16),
}

pub enum Status {
    Compiling,
    Compiled { warnings: usize },
    Failed(String),
}

/// Starts the worker and the file watcher feeding it, and requests the initial compilation.
pub fn spawn(world: PreviewWorld, picker: Picker, events: Sender<Event>) -> Sender<Request> {
    let (requests, inbox) = mpsc::channel();
    let (dependencies, watched) = mpsc::channel();

    let watcher_events = events.clone();
    let watcher_requests = requests.clone();
    thread::spawn(move || {
        if let Err(err) = watch(&watched, &watcher_requests) {
            let _ = watcher_events.send(Event::Status(Status::Failed(format!("watcher: {err}"))));
        }
    });

    let worker = Worker {
        world,
        picker,
        document: None,
        width: None,
        cache: HashMap::new(),
        dependencies,
        events,
    };
    thread::spawn(move || worker.run(&inbox));

    requests
        .send(Request::Recompile)
        .expect("the worker thread owns the receiver");
    requests
}

/// Waits for changes to the dependencies of the latest compilation and requests a recompile.
///
/// Runs in lockstep with the worker: after each compilation the worker sends its dependency
/// list, which is watched until the next change.
fn watch(dependencies: &Receiver<Vec<PathBuf>>, requests: &Sender<Request>) -> StrResult<()> {
    let mut watcher = Watcher::new(None)?;
    while let Ok(paths) = dependencies.recv() {
        watcher.update(paths)?;
        watcher.wait()?;
        if requests.send(Request::Recompile).is_err() {
            break;
        }
    }
    Ok(())
}

struct Worker {
    world: PreviewWorld,
    picker: Picker,
    /// The latest successfully compiled document.
    document: Option<PagedDocument>,
    width: Option<u16>,
    cache: HashMap<(u128, u16), Arc<SlicedProtocol>>,
    dependencies: Sender<Vec<PathBuf>>,
    events: Sender<Event>,
}

impl Worker {
    fn run(mut self, inbox: &Receiver<Request>) {
        // Coalesce queued requests, so that bursts of resizes or edits cause one render.
        while let Ok(first) = inbox.recv() {
            let mut recompile = false;
            for request in iter::once(first).chain(inbox.try_iter()) {
                match request {
                    Request::Recompile => recompile = true,
                    Request::Resize(width) => self.width = Some(width),
                }
            }
            if recompile {
                self.compile();
            }
            if let Err(err) = self.publish() {
                self.send(Event::Status(Status::Failed(format!("encoding: {err}"))));
            }
        }
    }

    fn compile(&mut self) {
        self.send(Event::Status(Status::Compiling));
        self.world.reset();
        let Warned { output, warnings } = typst::compile::<PagedDocument>(&self.world);
        typst::comemo::evict(10);
        // The watcher thread only stops when the worker does.
        let _ = self.dependencies.send(self.world.dependencies());

        let status = match output {
            Ok(document) => {
                self.document = Some(document);
                Status::Compiled {
                    warnings: warnings.len(),
                }
            }
            Err(errors) => Status::Failed(self.world.describe(&errors[0])),
        };
        self.send(Event::Status(status));
    }

    /// Sends the encoded pages of the current document at the current width.
    fn publish(&mut self) -> Result<(), Errors> {
        let (Some(document), Some(width)) = (&self.document, self.width) else {
            return Ok(());
        };
        let mut cache = HashMap::with_capacity(document.pages().len());
        let mut pages = Vec::with_capacity(document.pages().len());
        for page in document.pages() {
            let key = (hash128(page), width);
            let encoded = match self.cache.remove(&key).or_else(|| cache.get(&key).cloned()) {
                Some(encoded) => encoded,
                None => Arc::new(encode(&self.picker, page, width)?),
            };
            cache.insert(key, Arc::clone(&encoded));
            pages.push(encoded);
        }
        self.cache = cache;
        self.send(Event::Pages(pages));
        Ok(())
    }

    fn send(&self, event: Event) {
        // The UI is gone only while the process is exiting.
        let _ = self.events.send(event);
    }
}

/// Rasterizes `page` at exactly `width` cells wide and encodes it for the terminal.
fn encode(picker: &Picker, page: &Page, width: u16) -> Result<SlicedProtocol, Errors> {
    let width_px = f64::from(width) * f64::from(picker.font_size().width);
    let options = RenderOptions {
        pixel_per_pt: Scalar::new(width_px / page.frame.width().to_pt()),
        render_bleed: false,
    };
    let pixmap = typst_render::render(page, &options);
    let image = RgbaImage::from_raw(pixmap.width(), pixmap.height(), pixmap.take_demultiplied())
        .expect("a pixmap holds width * height RGBA pixels");
    SlicedProtocol::new(picker, image.into(), None)
}
