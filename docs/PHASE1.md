# Phase 1 — The app

Phase 0 proved unbounded zoom is fast and bounded by what is on screen. Phase 1
builds the real app (`crates/og-paper`) on `ogpaper-core`. The spike stays as a
benchmark.

| Milestone | Scope |
| --- | --- |
| **M1 · App + ink + files** | App shell (winit + wgpu + egui) · toolbar · pen / marker / highlighter with pressure · color swatches + picker · width · stroke eraser · undo/redo · autosave to `.ogp` (SQLite) · New / Open / Save As |
| **M2 · Stickers** | Paste or drag-drop PNG / JPG / SVG (clipboard, files, browser paste) · move / resize / rotate · images anchored to cells like strokes · SVGs re-rasterized per zoom so they stay crisp at any depth |
| **M3 · Shapes + select** | Line, arrow, rectangle, ellipse, polygon (stroke + fill) · lasso select · move / resize / duplicate / delete across zoom levels |
| **M4 · Textures + brushes** | Pencil grain, fountain pen (tilt/speed), calligraphy nib, spray · brush presets |
| **M5 · Scale + polish** | Load-on-demand from `.ogp`, GPU paging, thumbnails, compact cells, quantized points · layers · bookmarks · SVG/PNG/PDF export |

Crates:

- `ogpaper-core` — model, camera, visibility, history (undo/redo), hit-testing.
- `ogpaper-file` — `.ogp` SQLite reader/writer (native; web storage later).
- `og-paper` — the app: tools, UI, renderer, platform glue (desktop, Android, web).
