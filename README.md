# OG Paper

**An open-source, truly endless canvas — infinite pan, infinite zoom — that runs on every device and saves your work in a format you own.**

Write a sentence inside the dot of an "i", then zoom out until a whole notebook is a speck. Pick up the same canvas on your Windows laptop, Android phone, iPad or a browser. And if this project ever stops, your files still open: the format is openly documented, stored in SQLite, and readable with a tiny standard-library Python script.

> **Status: Phase 1 (the app), early alpha.** Drawing, files and the web app work on Windows, macOS, Linux, Android and the browser — see [What works today](#what-works-today). The Phase 0 spike flies from 10^-0.6 to 10^49 zoom over a 1M-stroke canvas at ~6,000 fps on desktop ([`docs/PHASE0.md`](docs/PHASE0.md)). Design spec: [`docs/DESIGN.md`](docs/DESIGN.md).

## Why

Proprietary infinite-canvas apps such as Endless Paper keep your work in undocumented formats, on one platform, behind a subscription. If the company changes course, years of notes can become unreadable. OG Paper exists so that can't happen.

## Goals

- **Truly endless** — unbounded zoom depth, not just 0.1×–10×. Only what's visible and big enough to see gets drawn.
- **Everywhere** — Windows, macOS, Linux, Android, iOS/iPadOS and the web, on phones, tablets and desktops.
- **Fast** — 2024 flagship phones are the performance bar: 120 fps pan/zoom, low-latency ink.
- **Files you own** — one `.ogp` file per canvas, open spec, plus SVG/PDF/PNG/JSON and self-contained HTML exports.
- **Sync without a company** — multi-device sync through a folder you already sync (iCloud, Google Drive, Dropbox, Syncthing) or a relay you self-host for free.

## Try it

- **Try mode:** https://omegagiven.github.io/OG-paper/try/ — the real app on a demo canvas 10^45 deep, with a guided tour.
- **Web:** https://omegagiven.github.io/OG-paper/app/ — then “Install app” / “Add to Home screen” for full screen.
  The web app autosaves in your browser and has bookmarks (fly to any saved view), a timeline (every stroke is
  time-stamped; scrub, replay or restore any moment) and offline copies (download / load a `.ogpt` file).
- **Windows, macOS, Linux, Android:** [releases page](https://github.com/OmegaGiven/OG-paper/releases)
  (every push to `main` also produces builds under the CI run's artifacts).

## What works today

- **Ink:** pen with pressure, marker, highlighter, stroke eraser, eyedropper, color dial with custom colors, per-brush width, unlimited undo/redo.
- **Endless canvas:** pan and zoom with no limit; content at any depth stays exact.
- **Controls:** round buttons that fan out like radial menus: tools (bottom right), color, ↩ undo / ↪ redo, and ⚙ settings (top right) for New canvas, Open, Save copy, Home, Bookmarks, Timeline, Full screen and the tour.
- **Files:** desktop and Android autosave to a `.ogp` file (SQLite) with New / Open / Save As. The web app autosaves in your browser and saves / opens `.ogpt` offline copies.
- **Bookmarks** (web): save a view, then fly back to it with one tap, across any zoom depth.
- **Timeline** (web): every stroke is time-stamped; scrub or play back the canvas as it was at any moment, and restore it.
- **Try mode** (web): a demo canvas with 15 worlds nested inside dots, down to 10^45, and a guided checklist.

File formats (`.ogp` 0.1, the timeline log, bookmarks, `.ogpt` v1) are documented in [`docs/DESIGN.md`](docs/DESIGN.md#implemented-today-ogp-format-01).

## Coming next

- **Save to / open from** the cloud: Save and Open fan out to device, Google Drive, Dropbox or OneDrive, signed in from the page with no project server.
- `.ogpt` copies in the desktop app, and `.ogp` in the web app, so both share one format; bookmarks and the timeline on desktop.
- Stickers (images), shapes and lasso select, textured brushes, layers, export, multi-device sync — see [`docs/PHASE1.md`](docs/PHASE1.md) and the [to-do list](docs/DESIGN.md#future-to-do).

## Stack

Rust core · wgpu renderer · winit · egui · SQLite. See [`docs/DESIGN.md`](docs/DESIGN.md) for architecture, file format, sync design and roadmap.

## Contributing

The repo is public from day one and help is welcome — especially people with stylus hardware (Apple Pencil, S Pen, Wacom) who can test ink feel. See [`CONTRIBUTING.md`](CONTRIBUTING.md). Commits must be signed off (DCO).

## License

- App code: [Mozilla Public License 2.0](LICENSE)
- File-format spec (`spec/`): [Creative Commons Attribution 4.0](spec/LICENSE)
- Reference reader and sample files (`tools/`, when added): MIT

Created by **OmegaGiven**. See [`AUTHORS`](AUTHORS) and [`TRADEMARKS.md`](TRADEMARKS.md).
