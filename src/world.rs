//! The Typst [`World`] that serves the previewed project from the file system.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use color_eyre::eyre::{Result, WrapErr, eyre};
use typst::diag::{FileResult, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World, WorldExt};
use typst_kit::datetime::Time;
use typst_kit::downloader::SystemDownloader;
use typst_kit::files::{FileStore, FsRoot, SystemFiles};
use typst_kit::fonts::{self, FontStore};
use typst_kit::packages::SystemPackages;

pub struct PreviewWorld {
    library: LazyHash<Library>,
    /// Scanned on first access, so that startup errors surface before the scan.
    fonts: LazyLock<FontStore, fn() -> FontStore>,
    files: FileStore<SystemFiles>,
    main: FileId,
    now: Time,
}

impl PreviewWorld {
    /// Creates a world whose project root is the directory containing `main`.
    pub fn new(main: &Path) -> Result<Self> {
        let main = main
            .canonicalize()
            .wrap_err_with(|| format!("cannot open {}", main.display()))?;
        let root = main
            .parent()
            .ok_or_else(|| eyre!("{} has no parent directory", main.display()))?
            .to_path_buf();
        let vpath = VirtualPath::virtualize(&root, &main).map_err(|err| eyre!("{err}"))?;

        let user_agent = concat!("typeek/", env!("CARGO_PKG_VERSION"));
        let packages = SystemPackages::new(SystemDownloader::new(user_agent));

        Ok(Self {
            library: LazyHash::new(Library::builder().build()),
            fonts: LazyLock::new(load_fonts),
            files: FileStore::new(SystemFiles::new(FsRoot::new(root), packages)),
            main: RootedPath::new(VirtualRoot::Project, vpath).intern(),
            now: Time::system(),
        })
    }

    /// Marks all files and the current time as stale before a recompilation.
    pub fn reset(&mut self) {
        self.files.reset();
        self.now.reset();
    }

    /// File system paths accessed by the most recent compilation.
    pub fn dependencies(&mut self) -> Vec<PathBuf> {
        let (loader, ids) = self.files.dependencies();
        ids.filter_map(|id| loader.resolve(id).ok()).collect()
    }

    /// Formats a diagnostic as `path:line:column: message`.
    pub fn describe(&self, diagnostic: &SourceDiagnostic) -> String {
        let location = diagnostic.span.id().and_then(|id| {
            let range = self.range(diagnostic.span)?;
            let source = self.source(id).ok()?;
            let (line, column) = source.lines().byte_to_line_column(range.start)?;
            Some(format!(
                "{}:{}:{}: ",
                id.vpath().get_with_slash(),
                line + 1,
                column + 1
            ))
        });
        format!("{}{}", location.unwrap_or_default(), diagnostic.message)
    }
}

fn load_fonts() -> FontStore {
    let mut store = FontStore::new();
    store.extend(fonts::system());
    store.extend(fonts::embedded());
    store
}

impl World for PreviewWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.fonts.book()
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.files.source(id)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files.file(id)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        self.now.today(offset)
    }
}
