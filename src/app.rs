//! UI state, input handling, and drawing.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use color_eyre::eyre::{Result, eyre};
use ratatui::DefaultTerminal;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self as terminal_event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect, Size};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::Widget;
use ratatui_image::picker::ProtocolType;
use ratatui_image::sliced::{SignedPosition, SlicedImage, SlicedProtocol};

use crate::event::Event;
use crate::strip::Strip;
use crate::worker::{Request, Status};

/// Page width as a percentage of the viewport width.
const ZOOM_DEFAULT: u16 = 100;
const ZOOM_MIN: u16 = 10;
const ZOOM_STEP: u16 = 10;

pub struct App {
    title: String,
    protocol: ProtocolType,
    events: Receiver<Event>,
    worker: Sender<Request>,
    pages: Vec<Arc<SlicedProtocol>>,
    strip: Strip,
    /// First strip row shown at the top of the viewport.
    scroll: u32,
    /// Size of the area pages are drawn into.
    viewport: Size,
    zoom: u16,
    requested_width: Option<u16>,
    status: Status,
    dirty: bool,
    exit: bool,
}

impl App {
    pub fn new(
        title: String,
        protocol: ProtocolType,
        worker: Sender<Request>,
        events: Receiver<Event>,
    ) -> Self {
        Self {
            title,
            protocol,
            events,
            worker,
            pages: Vec::new(),
            strip: Strip::default(),
            scroll: 0,
            viewport: Size::default(),
            zoom: ZOOM_DEFAULT,
            requested_width: None,
            status: Status::Compiling,
            dirty: true,
            exit: false,
        }
    }

    /// Runs until the user quits.
    ///
    /// Blocks until something happens, applies every event queued by then, and draws at
    /// most once per batch. Input that arrives faster than frames can be drawn (key repeat,
    /// resize drags) is thereby merged into a single frame instead of one frame per event.
    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let size = terminal.size()?;
        self.resize(Rect::new(0, 0, size.width, size.height))?;
        while !self.exit {
            if self.dirty {
                terminal.draw(|frame| frame.render_widget(&self, frame.area()))?;
                self.dirty = false;
            }
            let event = self.events.recv()?;
            self.handle(event)?;
            while let Ok(event) = self.events.try_recv() {
                self.handle(event)?;
            }
        }
        Ok(())
    }

    fn handle(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Input(input) => match input? {
                terminal_event::Event::Key(key) if key.is_press() => self.handle_key(key)?,
                terminal_event::Event::Resize(width, height) => {
                    self.resize(Rect::new(0, 0, width, height))?;
                }
                _ => {}
            },
            Event::Pages(pages) => self.set_pages(pages),
            Event::Status(status) => {
                self.status = status;
                self.dirty = true;
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        let rows = i64::from(self.viewport.height);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if ctrl => self.exit = true,
            KeyCode::Char('d') if ctrl => self.scroll_by(rows / 2),
            KeyCode::Char('u') if ctrl => self.scroll_by(-rows / 2),
            KeyCode::Char('f') if ctrl => self.scroll_by(rows),
            KeyCode::Char('b') if ctrl => self.scroll_by(-rows),
            KeyCode::Char('q') | KeyCode::Esc => self.exit = true,
            KeyCode::Char('j') | KeyCode::Down => self.scroll_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_by(-1),
            KeyCode::Char(' ') | KeyCode::PageDown => self.scroll_by(rows),
            KeyCode::PageUp => self.scroll_by(-rows),
            KeyCode::Char('g') | KeyCode::Home => self.scroll_to(0),
            KeyCode::Char('G') | KeyCode::End => self.scroll_to(u32::MAX),
            KeyCode::Char('n') | KeyCode::Char(']') => self.next_page(),
            KeyCode::Char('p') | KeyCode::Char('[') => self.previous_page(),
            KeyCode::Char('+') | KeyCode::Char('=') => self.set_zoom(self.zoom + ZOOM_STEP)?,
            KeyCode::Char('-') => self.set_zoom(self.zoom.saturating_sub(ZOOM_STEP))?,
            KeyCode::Char('0') => self.set_zoom(ZOOM_DEFAULT)?,
            _ => {}
        }
        Ok(())
    }

    fn resize(&mut self, area: Rect) -> Result<()> {
        let [preview, _] = split(area);
        self.viewport = preview.as_size();
        self.scroll_to(self.scroll);
        self.dirty = true;
        self.request_width()
    }

    fn set_zoom(&mut self, zoom: u16) -> Result<()> {
        self.zoom = zoom.clamp(ZOOM_MIN, ZOOM_DEFAULT);
        self.dirty = true;
        self.request_width()
    }

    /// Asks the worker for pages fitting the current viewport and zoom, unless already asked.
    fn request_width(&mut self) -> Result<()> {
        let width = (u32::from(self.viewport.width) * u32::from(self.zoom) / 100).max(1);
        let width = u16::try_from(width).expect("at most the viewport width");
        if self.requested_width != Some(width) {
            self.worker
                .send(Request::Resize(width))
                .map_err(|_| eyre!("the compilation worker stopped"))?;
            self.requested_width = Some(width);
        }
        Ok(())
    }

    /// Replaces the pages, keeping the same document position in view.
    fn set_pages(&mut self, pages: Vec<Arc<SlicedProtocol>>) {
        let anchor = self.strip.anchor(self.scroll);
        self.strip = Strip::new(pages.iter().map(|page| u32::from(page.size().height)));
        self.pages = pages;
        self.scroll_to(anchor.map_or(0, |anchor| self.strip.row(anchor)));
        self.dirty = true;
    }

    fn scroll_by(&mut self, delta: i64) {
        let row = (i64::from(self.scroll) + delta).max(0);
        self.scroll_to(u32::try_from(row).unwrap_or(u32::MAX));
    }

    fn scroll_to(&mut self, row: u32) {
        let row = row.min(self.strip.max_scroll(self.viewport.height.into()));
        if row != self.scroll {
            self.scroll = row;
            self.dirty = true;
        }
    }

    fn next_page(&mut self) {
        let next = self.strip.page_at(self.scroll) + 1;
        if next < self.strip.len() {
            self.scroll_to(self.strip.top(next));
        }
    }

    fn previous_page(&mut self) {
        if self.strip.len() == 0 {
            return;
        }
        let page = self.strip.page_at(self.scroll);
        let top = self.strip.top(page);
        let row = if self.scroll > top || page == 0 {
            top
        } else {
            self.strip.top(page - 1)
        };
        self.scroll_to(row);
    }

    fn render_status(&self, area: Rect, buf: &mut Buffer) {
        let page = if self.strip.len() == 0 {
            0
        } else {
            self.strip.page_at(self.scroll) + 1
        };
        let info = Line::from(format!(
            " {} │ {}/{} │ {}% │ {:?} ",
            self.title,
            page,
            self.strip.len(),
            self.zoom,
            self.protocol,
        ))
        .reversed();
        let status = match &self.status {
            Status::Compiling => Line::from("compiling…").dim(),
            Status::Compiled { warnings: 0 } => Line::from("ok").green(),
            Status::Compiled { warnings } => Line::from(format!("{warnings} warnings")).yellow(),
            Status::Failed(message) => {
                Line::from(message.as_str()).style(Style::new().fg(Color::Red))
            }
        };
        let [info_area, status_area] =
            Layout::horizontal([Constraint::Length(info.width() as u16), Constraint::Fill(1)])
                .spacing(1)
                .areas(area);
        info.render(info_area, buf);
        status.right_aligned().render(status_area, buf);
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [preview, status] = split(area);
        for (page, offset) in self.strip.visible(self.scroll, preview.height.into()) {
            let protocol = &self.pages[page];
            // Visible pages start less than one page height above the viewport, which only
            // overflows `SignedPosition` for pages over 32767 rows tall.
            let Ok(y) = i16::try_from(offset) else {
                continue;
            };
            let x = (preview.width.saturating_sub(protocol.size().width) / 2) as i16;
            SlicedImage::new(protocol, SignedPosition { x, y }).render(preview, buf);
        }
        self.render_status(status, buf);
    }
}

/// Splits the screen into the page preview and the status line.
fn split(area: Rect) -> [Rect; 2] {
    Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area)
}
