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

- **Ink:** pen with pressure, marker, highlighter; solid, dashed or dotted; any opacity; stroke eraser, eyedropper, color dial with custom colors, per-brush width, unlimited undo/redo.
- **Shapes:** one Shapes tool with rectangle, ellipse, diamond, triangle, star, polygon, line and arrow, and Excalidraw-style options: hachure / cross-hatch / zigzag / solid fill, fill color, stroke width and style, sloppiness (clean to cartoon), sharp or round edges, curved and elbow lines, nine arrowheads, opacity.
- **Text:** 11 bundled open-licensed fonts (hand-drawn, marker, sans, serif, mono, display) plus three single-stroke fonts, or add your own `.ttf` / `.otf`; sizes, alignment, color, opacity. Text is ink (letters are filled outlines), so it stays sharp at any zoom.
- **Select and edit:** tap or drag a box to select; move, resize, rotate, flip, duplicate, delete, bring to front / send to back, copy / paste, and restyle anything from the tool panel; double-tap a text to edit it. Each edit is one undo step.
- **Endless canvas:** pan and zoom with no limit; content at any depth stays exact.
- **Controls:** round buttons that fan out like radial menus, in rings when there are many items: tools (bottom right), ↩ undo / ↪ redo, a tool panel (top left, collapsible) with the current tool's or the selection's settings, and ⚙ settings (top right) for New canvas, Open, Save copy, Home, Bookmarks, Timeline, Full screen and the tour. Keyboard shortcuts for every tool (see [`docs/DESIGN.md`](docs/DESIGN.md#where-it-stands-2026-10-01)).
- **Files:** desktop and Android autosave to a `.ogp` file (SQLite) with New / Open / Save As. The web app autosaves in your browser and saves / opens `.ogpt` offline copies.
- **Bookmarks** (web): save a view, then fly back to it with one tap, across any zoom depth.
- **Timeline** (web): every stroke is time-stamped; scrub or play back the canvas as it was at any moment, and restore it.
- **Try mode** (web): a demo canvas with 15 worlds nested inside dots, down to 10^45, and a guided checklist.

File formats (`.ogp` 0.2, shapes and text, the timeline log, bookmarks, `.ogpt` v2) are documented in [`docs/DESIGN.md`](docs/DESIGN.md#implemented-today-ogp-format-02).

## Coming next

- **Save to / open from** the cloud: Save and Open fan out to device, Google Drive, Dropbox or OneDrive, signed in from the page with no project server.
- `.ogpt` copies in the desktop app, and `.ogp` in the web app, so both share one format; bookmarks and the timeline on desktop.
- Stickers (images), lasso select, arrows that stick to shapes, textured brushes, layers, export, multi-device sync — see [`docs/PHASE1.md`](docs/PHASE1.md) and the [to-do list](docs/DESIGN.md#future-to-do).

## Stack

Rust core · wgpu renderer · winit · egui · SQLite. See [`docs/DESIGN.md`](docs/DESIGN.md) for architecture, file format, sync design and roadmap.

## Contributing

The repo is public from day one and help is welcome — especially people with stylus hardware (Apple Pencil, S Pen, Wacom) who can test ink feel. See [`CONTRIBUTING.md`](CONTRIBUTING.md). Commits must be signed off (DCO).

## License

- App code: [Mozilla Public License 2.0](LICENSE)
- File-format spec (`spec/`): [Creative Commons Attribution 4.0](spec/LICENSE)
- Reference reader and sample files (`tools/`, when added): MIT

Created by **OmegaGiven**. See [`AUTHORS`](AUTHORS) and [`TRADEMARKS.md`](TRADEMARKS.md).
