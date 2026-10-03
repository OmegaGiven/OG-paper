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

## Run your own server

One small container hosts all your pages on your own hardware, so everyone
on your network (or tailnet) can open and draw on them together, and each
device keeps its own copy that syncs when it reconnects. It needs no
account and no cloud.

### The short version (any machine with Docker)

```sh
git clone https://github.com/OmegaGiven/OG-paper.git
cd OG-paper
docker compose up -d --build      # first build takes a few minutes
docker compose logs og-paper      # copy a "Server link" from here
```

The log shows two **server links**: one that can make and change pages, and
a view-only one. Treat them like passwords. Your pages live in
`./og-paper-data` (mounted at `/data` in the container). The server listens
on port **8991**.

Without compose:

```sh
docker build -t og-paper .
docker run -d --name og-paper --restart unless-stopped \
  -p 8991:8991 -v /path/on/disk/og-paper:/data og-paper
docker logs og-paper
```

The links name the container's internal address. To get links with the
address people actually use, add `--public ws://<your-server-ip>:8991` to
the command (see the commented `command:` line in
[`docker-compose.yml`](docker-compose.yml)), or just swap the address in the
link by hand. Browsers on the public web app (an `https://` page) can only
connect to a `wss://` address. Put the server behind a TLS proxy (Tailscale
serve, Caddy, Nginx Proxy Manager, Traefik) and use `--public
wss://your-host`. The desktop and Android apps connect to plain `ws://`
fine.

### On a NAS or home server

There is no prebuilt image yet, so the image is built from this repository
once (on the NAS itself, or on a PC and then copied over, see below).

- **Synology (DSM 7.2+, Container Manager):** copy the repository to a
  shared folder (e.g. `/volume1/docker/og-paper`). In **Container Manager >
  Project > Create**, choose that folder as the path; it picks up
  `docker-compose.yml`. Build and start it. Open the container's **Log**
  tab for the server links. To use a NAS folder for the pages, change
  `./og-paper-data` to e.g. `/volume1/docker/og-paper/data`.
- **QNAP (Container Station 3):** **Applications > Create**, paste the
  contents of `docker-compose.yml` (with `build: .` replaced by a built
  image, see below, or create it from the repository folder over SSH with
  `docker compose up -d --build`). Map `/data` to a folder such as
  `/share/Container/og-paper`.
- **Unraid:** over SSH, `git clone` into `/mnt/user/appdata/og-paper-src` and
  run `docker build -t og-paper .` there. Then **Docker > Add Container**:
  Repository `og-paper`, port 8991 to 8991, path `/data` to
  `/mnt/user/appdata/og-paper`. The server links are in the container's log.
- **TrueNAS SCALE:** build the image in a shell (`docker build -t og-paper
  .`), then **Apps > Discover Apps > Custom App**: image `og-paper` (pull
  policy: never), port 8991, host path for `/data` on a dataset (e.g.
  `/mnt/tank/apps/og-paper`).
- **Raspberry Pi, a Linux box, a VPS:** the short version above works as is
  (64-bit Raspberry Pi OS / any arm64 or x86-64 Linux with Docker).
- **Without Docker:** download (or `cargo build --release -p og-paper`) the
  Linux build and run `og-paper --serve-dir ~/og-paper-pages`. A systemd
  service with `Restart=always` keeps it up.

**Building on a PC for a slow NAS:** build for the NAS's CPU, then load it
there:

```sh
docker buildx build --platform linux/arm64 -t og-paper --load .   # or linux/amd64
docker save og-paper | gzip > og-paper.tar.gz
# copy og-paper.tar.gz to the NAS, then on the NAS:
docker load < og-paper.tar.gz
```

Open port 8991 on the NAS firewall for your LAN. For access from outside, a
VPN or Tailscale is safer than opening the port to the internet.

## Connect to a page

Every way works from the ⚙ settings menu:

1. **Your server's pages:** ⚙ > **Pages** > paste a server link under
   *Servers* > **Add**. Its pages appear: **Open** one and you're drawing on
   it with everyone else; **+ Page** makes a new one. The app keeps a copy
   of each page you open; reopening it from *On this device* reconnects by
   itself, and anything you drew offline goes up.
2. **One page, live, no server:** ⚙ > **Share live** > **Host this canvas**
   (desktop) and send someone the edit or view link; they paste it under
   *Join* (or open the browser link). In a browser: **Host in this
   browser** makes one-time invite links.
3. **Swap copies:** ⚙ > **Save copy**, send the file, and they use ⚙ >
   **Merge copy**: both sets of changes combine. **Sync folder** does this
   automatically through Syncthing, Dropbox, Drive or a USB stick.

More detail, including the relay for people who are never online at the
same time and how the end-to-end encryption works, is in
[`docs/SHARING.md`](docs/SHARING.md).

## What works today

- **Brush:** a simple line (width, pressure, solid / dashed / dotted, opacity), or **Advanced**: a GIMP-style brush engine with 15 looks (pencil, charcoal, bristle brush, watercolor, airbrush, spray, calligraphy, sketchy, jagged, neon, chain, stitches, felt tip, rainbow, ink) and every setting behind them: 13 tip shapes, hardness, spacing, angle, roundness, pressure and speed dynamics, taper, fade, scatter, jitter, sketchy wobble, flow, paper grain, color jitter, hue cycle and fade-to color. Strokes stay vector and sharp at any zoom.
- **Texture:** splotches, spatter, leaves, stars, hearts, patterns (dots, hatch, weave, grain, ...), paper grain and sponge, with the same engine settings.
- **Ink tools:** highlighter, stroke eraser, eyedropper, color dial with custom colors, unlimited undo/redo.
- **Bucket fill:** tap inside an outline to fill it with real vector ink tucked under the lines (sharp at any zoom), optionally closing small gaps in the outline.
- **Shapes:** one Shapes tool with rectangle, ellipse, diamond, triangle, star, polygon, line and arrow, and Excalidraw-style options: hachure / cross-hatch / zigzag / solid fill, fill color (or one color for stroke and fill), stroke width and style, sloppiness (clean to cartoon), sharp or round edges, curved and elbow lines, nine arrowheads, opacity.
- **Text:** 11 bundled open-licensed fonts (hand-drawn, marker, sans, serif, mono, display) plus three single-stroke fonts, or add your own `.ttf` / `.otf`; sizes, alignment, color, opacity. Text is ink (letters are filled outlines), so it stays sharp at any zoom.
- **Pictures, PDFs and tables:** paste (Ctrl+V or the Paste button, for phones), drag-drop or insert pictures (PNG, JPEG, GIF, WebP, SVG) and crop them in place (non-destructive); import PDFs, each page a picture to write on; paste spreadsheet cells or a Markdown table and get a real table you can restyle and edit; pasted text becomes text.
- **Library:** save any selection as a sticker and place copies of it on any canvas, at any zoom.
- **Diagrams:** in diagram mode, lines and arrows snap to shapes, texts and pictures and follow them when they move.
- **Saved tools:** Minecraft-style toolbars (keys 1–9) of tools with their settings: a red 2 px pen, a dashed arrow, a font at a size... Keep as many named toolbars as you like and switch between them (`[` / `]`), plus an inventory that grows as you fill it. Tap an empty slot to save the current tool.
- **Select and edit:** tap, drag a box or draw a lasso to select; move, resize, rotate, flip, duplicate, delete, bring to front / send to back, copy / cut / paste, and restyle anything from the tool panel; double-tap a text or table to edit it. Each edit is one undo step.
- **Endless canvas:** pan and zoom with no limit; content at any depth stays exact.
- **Controls:** round buttons that fan out like radial menus, in rings when there are many items: tools (bottom right, with undo and redo at the front of the fan; put them in a toolbar slot to keep them at hand), a tool panel (bottom left, collapsible) with the current tool's or the selection's settings, and ⚙ settings (top right) for New canvas, Open, Save copy, Export, Paste, Library, Insert picture / PDF, Search text, Home, Bookmarks, Timeline, Grid, Diagram, Edit layout, Full screen and the tour. **Edit layout** moves any of them anywhere; fans open toward the middle of the screen from wherever their button is.
- **Find and show:** search all text on the canvas and fly to it; lines or dot grid behind the ink; export the view or the selection as PNG, JPEG, SVG or PDF. Keyboard shortcuts for every tool (see [`docs/DESIGN.md`](docs/DESIGN.md#where-it-stands-2026-10-02)).
- **Files:** desktop and Android autosave to a `.ogp` file (SQLite) with New / Open / Save As. The web app autosaves in your browser and saves / opens `.ogpt` offline copies.
- **Bookmarks** (web): save a view, then fly back to it with one tap, across any zoom depth.
- **Timeline** (web): every stroke is time-stamped; scrub or play back the canvas as it was at any moment, narrow it to a window of time with the left handle to see one session's work on its own, and restore a moment.
- **Try mode** (web): a demo canvas with 15 worlds nested inside dots, down to 10^45, and a guided checklist.
- **Sharing and sync:** every copy of a canvas merges with the others — Merge copy, Save changes, a shared sync folder, live drawing with a host (desktop, headless `og-paper --serve`, or Docker), with no server between browsers (WebRTC), or through a relay (`og-paper --relay`) for people who come and go. Offline work applies when you reconnect; frames are sealed end to end and view links cannot edit. See [`docs/SHARING.md`](docs/SHARING.md).
- **Import canvas, dark mode:** bring another saved canvas in and drag it into place (exact at any depth); flip the whole screen dark.

File formats (`.ogp` 0.3, shapes, text, tables and pictures, the timeline log, bookmarks, `.ogpt` v3) are documented in [`docs/DESIGN.md`](docs/DESIGN.md#implemented-today-ogp-format-03).

## Coming next

- **Save to / open from** the cloud: Save and Open fan out to device, Google Drive, Dropbox or OneDrive, signed in from the page with no project server.
- `.ogpt` copies in the desktop app, and `.ogp` in the web app, so both share one format; bookmarks and the timeline on desktop.
- Vector SVG pictures, textured brushes, layers, frames and presenting — see [`docs/PHASE1.md`](docs/PHASE1.md) and the [to-do list](docs/DESIGN.md#future-to-do).

## Stack

Rust core · wgpu renderer · winit · egui · SQLite. See [`docs/DESIGN.md`](docs/DESIGN.md) for architecture, file format, sync design and roadmap.

## Contributing

The repo is public from day one and help is welcome — especially people with stylus hardware (Apple Pencil, S Pen, Wacom) who can test ink feel. See [`CONTRIBUTING.md`](CONTRIBUTING.md). Commits must be signed off (DCO).

## License

- App code: [Mozilla Public License 2.0](LICENSE)
- File-format spec (`spec/`): [Creative Commons Attribution 4.0](spec/LICENSE)
- Reference reader and sample files (`tools/`, when added): MIT

Created by **OmegaGiven**. See [`AUTHORS`](AUTHORS) and [`TRADEMARKS.md`](TRADEMARKS.md).
