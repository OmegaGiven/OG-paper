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
- **Bucket fill:** tap inside an outline to fill it with real vector ink tucked under the lines (sharp at any zoom), optionally closing small gaps in the outline.
- **Shapes:** one Shapes tool with rectangle, ellipse, diamond, triangle, star, polygon, line and arrow, and Excalidraw-style options: hachure / cross-hatch / zigzag / solid fill, fill color (or one color for stroke and fill), stroke width and style, sloppiness (clean to cartoon), sharp or round edges, curved and elbow lines, nine arrowheads, opacity.
- **Text:** 11 bundled open-licensed fonts (hand-drawn, marker, sans, serif, mono, display) plus three single-stroke fonts, or add your own `.ttf` / `.otf`; sizes, alignment, color, opacity. Text is ink (letters are filled outlines), so it stays sharp at any zoom.
- **Pictures, PDFs and tables:** paste (Ctrl+V or the Paste button, for phones), drag-drop or insert pictures (PNG, JPEG, GIF, WebP, SVG) and crop them in place (non-destructive); import PDFs, each page a picture to write on; paste spreadsheet cells or a Markdown table and get a real table you can restyle and edit; pasted text becomes text.
- **Library:** save any selection as a sticker and place copies of it on any canvas, at any zoom.
- **Diagrams:** in diagram mode, lines and arrows snap to shapes, texts and pictures and follow them when they move.
- **Saved tools:** Minecraft-style toolbars (keys 1–9) of tools with their settings: a red 2 px pen, a dashed arrow, a font at a size... Keep as many named toolbars as you like and switch between them (`[` / `]`), plus an inventory that grows as you fill it. Tap an empty slot to save the current tool.
- **Select and edit:** tap, drag a box or draw a lasso to select; move, resize, rotate, flip, duplicate, delete, bring to front / send to back, copy / cut / paste, and restyle anything from the tool panel; double-tap a text or table to edit it. Each edit is one undo step.
- **Endless canvas:** pan and zoom with no limit; content at any depth stays exact.
- **Controls:** round buttons that fan out like radial menus, in rings when there are many items: tools (bottom right), ↩ undo / ↪ redo, a tool panel (bottom left, collapsible) with the current tool's or the selection's settings, and ⚙ settings (top right) for New canvas, Open, Save copy, Export, Paste, Library, Insert picture / PDF, Search text, Home, Bookmarks, Timeline, Grid, Diagram, Edit layout, Full screen and the tour. **Edit layout** moves any of them anywhere; fans open toward the middle of the screen from wherever their button is.
- **Find and show:** search all text on the canvas and fly to it; lines or dot grid behind the ink; export the view or the selection as PNG, JPEG, SVG or PDF. Keyboard shortcuts for every tool (see [`docs/DESIGN.md`](docs/DESIGN.md#where-it-stands-2026-10-02)).
- **Files:** desktop and Android autosave to a `.ogp` file (SQLite) with New / Open / Save As. The web app autosaves in your browser and saves / opens `.ogpt` offline copies.
- **Bookmarks** (web): save a view, then fly back to it with one tap, across any zoom depth.
- **Timeline** (web): every stroke is time-stamped; scrub or play back the canvas as it was at any moment, narrow it to a window of time with the left handle to see one session's work on its own, and restore a moment.
- **Try mode** (web): a demo canvas with 15 worlds nested inside dots, down to 10^45, and a guided checklist.

File formats (`.ogp` 0.3, shapes, text, tables and pictures, the timeline log, bookmarks, `.ogpt` v3) are documented in [`docs/DESIGN.md`](docs/DESIGN.md#implemented-today-ogp-format-03).

## Coming next

- **Save to / open from** the cloud: Save and Open fan out to device, Google Drive, Dropbox or OneDrive, signed in from the page with no project server.
- `.ogpt` copies in the desktop app, and `.ogp` in the web app, so both share one format; bookmarks and the timeline on desktop.
- Vector SVG pictures, textured brushes, layers, frames and presenting, multi-device sync — see [`docs/PHASE1.md`](docs/PHASE1.md) and the [to-do list](docs/DESIGN.md#future-to-do).

## Stack

Rust core · wgpu renderer · winit · egui · SQLite. See [`docs/DESIGN.md`](docs/DESIGN.md) for architecture, file format, sync design and roadmap.

## Contributing

The repo is public from day one and help is welcome — especially people with stylus hardware (Apple Pencil, S Pen, Wacom) who can test ink feel. See [`CONTRIBUTING.md`](CONTRIBUTING.md). Commits must be signed off (DCO).

## License

- App code: [Mozilla Public License 2.0](LICENSE)
- File-format spec (`spec/`): [Creative Commons Attribution 4.0](spec/LICENSE)
- Reference reader and sample files (`tools/`, when added): MIT

Created by **OmegaGiven**. See [`AUTHORS`](AUTHORS) and [`TRADEMARKS.md`](TRADEMARKS.md).
