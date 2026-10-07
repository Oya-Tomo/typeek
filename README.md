# typeek

A terminal previewer for [Typst](https://typst.app) documents, built with
[Ratatui](https://ratatui.rs) and [ratatui-image](https://github.com/ratatui/ratatui-image).

typeek compiles a Typst file, renders its pages as images in the terminal, and recompiles
whenever the file or any of its dependencies change. Pages are laid out as one continuous
strip that you can scroll through like a PDF viewer.

## Requirements

- A terminal with a graphics protocol: Kitty, Sixel, or iTerm2.
  Other terminals fall back to Unicode half blocks, which are low resolution.
- Rust 1.92 or newer to build.

## Installation

```sh
cargo install --git https://github.com/Oya-Tomo/typeek
```

Or from a local checkout:

```sh
cargo install --path .
```

The first build takes a few minutes, because it compiles Typst and a vendored OpenSSL
(used to download Typst packages).

## Usage

```sh
typeek path/to/main.typ
```

The directory containing the file is the project root. Packages from Typst Universe are
downloaded on demand, and system fonts as well as Typst's embedded fonts are available.

### Keys

| Key                            | Action                    |
| ------------------------------ | ------------------------- |
| `j` / `↓`, `k` / `↑`           | Scroll one row            |
| `Ctrl-d`, `Ctrl-u`             | Scroll half a screen      |
| `Space` / `PageDown` / `Ctrl-f` | Scroll one screen down    |
| `PageUp` / `Ctrl-b`            | Scroll one screen up      |
| `g` / `Home`, `G` / `End`      | Jump to the start or end  |
| `n` / `]`, `p` / `[`           | Next or previous page     |
| `+` / `=`, `-`, `0`            | Zoom in, zoom out, reset  |
| `q` / `Esc` / `Ctrl-c`         | Quit                      |

The status line shows the file, the current page, the zoom level, the graphics protocol,
and the compilation result. If compilation fails, the last successful render stays on screen
and the first error is shown with its location.

## How it works

- A background worker compiles the document and rasterizes each page to exactly the
  requested width in terminal cells. Each page is encoded once as a ratatui-image
  `SlicedProtocol`. Pages are cached by content hash, so an edit only re-renders the pages
  it changed.
- Scrolling moves in whole rows. Pages that straddle the viewport edge are clipped by
  `SlicedImage`, so scrolling never re-encodes any image.
- The UI waits on a single event channel, applies every queued event, and draws at most once
  per batch. Key repeat and resize drags therefore do not cause one frame per event.
- File watching uses `typst-kit`'s watcher on the dependencies of the latest compilation.

## Limitations

- Zoom is limited to the window width; there is no horizontal scrolling.
- Resizing the window re-renders every page.
- Changing the terminal font size while running is not picked up.
- Mouse scrolling is not supported yet.
