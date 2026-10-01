# OG Paper

**An open-source, truly endless canvas — infinite pan, infinite zoom — that runs on every device and saves your work in a format you own.**

Write a sentence inside the dot of an "i", then zoom out until a whole notebook is a speck. Pick up the same canvas on your Windows laptop, Android phone, iPad or a browser. And if this project ever stops, your files still open: the format is openly documented, stored in SQLite, and readable with a tiny standard-library Python script.

> **Status: Phase 0 (zoom spike).** A prototype flies from 10^-0.6 to 10^49 zoom over a 1M-stroke canvas at ~6,000 fps on desktop — see [`docs/PHASE0.md`](docs/PHASE0.md). Design spec: [`docs/DESIGN.md`](docs/DESIGN.md).

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

## Planned stack

Rust core · wgpu renderer · winit · egui · SQLite. See [`docs/DESIGN.md`](docs/DESIGN.md) for architecture, file format, sync design and roadmap.

## Contributing

The repo is public from day one and help is welcome — especially people with stylus hardware (Apple Pencil, S Pen, Wacom) who can test ink feel. See [`CONTRIBUTING.md`](CONTRIBUTING.md). Commits must be signed off (DCO).

## License

- App code: [Mozilla Public License 2.0](LICENSE)
- File-format spec (`spec/`): [Creative Commons Attribution 4.0](spec/LICENSE)
- Reference reader and sample files (`tools/`, when added): MIT

Created by **OmegaGiven**. See [`AUTHORS`](AUTHORS) and [`TRADEMARKS.md`](TRADEMARKS.md).
