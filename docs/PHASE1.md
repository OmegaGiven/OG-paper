# Phase 1 — The app

Phase 0 proved unbounded zoom is fast and bounded by what is on screen. Phase 1
builds the real app (`crates/og-paper`) on `ogpaper-core`. The spike stays as a
benchmark.

| Milestone | Scope | Status |
| --- | --- | --- |
| **M1 · App + ink + files** | App shell (winit + wgpu + egui) · toolbar · pen / marker / highlighter with pressure · color swatches + picker · width · stroke eraser · undo/redo · autosave to `.ogp` (SQLite) · New / Open / Save As | Done |
| **M2 · Stickers** | Paste or drag-drop PNG / JPG / SVG (clipboard, files, browser paste) · move / resize / rotate · images anchored to cells like strokes · SVGs re-rasterized per zoom so they stay crisp at any depth | Done except vector SVG: paste / drop / insert pictures (SVG rasterized at 2x on the web), move / resize / rotate / flip, stored once per file; plus pasted tables |
| **M3 · Shapes + select** | Line, arrow, rectangle, ellipse, polygon (stroke + fill) · lasso select · move / resize / duplicate / delete across zoom levels | Done except lasso: 8 shapes with Excalidraw-style fills and styles, text, box select, move / resize / rotate / flip / duplicate / delete / reorder / restyle |
| **M4 · Textures + brushes** | Pencil grain, fountain pen (tilt/speed), calligraphy nib, spray · brush presets | |
| **M5 · Scale + polish** | Load-on-demand from `.ogp`, GPU paging, thumbnails, compact cells, quantized points · layers · bookmarks · SVG/PNG/PDF export | Bookmarks done on the web |

Also done, beyond the milestones:

- **Radial controls:** tool fan in rings, undo/redo, a collapsible tool panel (per-tool settings, the color dial, selection actions), and a ⚙ settings fan for canvas commands (replaced the ☰ menu); icons drawn as recognizable objects.
- **Ink styles:** dashed and dotted strokes, opacity.
- **Saved tools:** a quick bar (keys 1–9) and an inventory of tools with their settings.
- **Tool panel** moved to the bottom left; shapes can share one color for stroke and fill.
- **Text tool:** 11 bundled outline fonts, three single-stroke fonts, and your own TTF / OTF fonts.
- **Web app:** browser autosave, `.ogpt` offline copies (download / open), bookmarks with fly-to, a timeline (scrub, play back, restore), and try mode at `/try/` with a demo canvas 10^45 deep and a guided tour.
- **Builds:** Windows, macOS, Linux, Android and web in CI; web deployed to GitHub Pages.

Formats are documented in [`DESIGN.md`](DESIGN.md#implemented-today-ogp-format-03); the to-do list (cloud save/open and more) is in [`DESIGN.md`](DESIGN.md#future-to-do).

Crates:

- `ogpaper-core` — model, camera, visibility, history (undo/redo), hit-testing.
- `ogpaper-file` — `.ogp` SQLite reader/writer (native; web storage later).
- `og-paper` — the app: tools, UI, renderer, platform glue (desktop, Android, web), timeline, bookmarks, `.ogpt` snapshots, the try-mode demo canvas.
