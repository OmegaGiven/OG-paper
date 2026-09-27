# Phase 0 — Zoom spike

Goal: prove unbounded zoom before any product work. Throwaway app code; the
`ogpaper-core` crate carries forward.

**Gate:** no jitter across a 10^30× zoom range; 120 fps with 1M strokes in the
canvas, on desktop and the reference phone (Galaxy S24).

## Status (2026-09-26)

| Check | Result |
| --- | --- |
| Zoom range | 10^-0.6 → 10^49 and back (40 nested scenes, 162 levels), no precision loss |
| Jitter | Point under cursor stays within 10^-6 px through zooms at 10^36; 2,000 sub-pixel pans drift < 10^-6 px (`camera` tests) |
| Lossless round trip | Zoom in 2^100 (~10^30) and back: same world position within 10^-9 of a cell |
| Desktop, 1M strokes (RX 6600 XT, 1692×1440, vsync off) | avg 0.17 ms/frame (~6,000 fps), p99 0.24 ms, visibility query p99 18 µs, max 98 µs |
| Visible set bounded | Worst frame: 2,676 strokes, ~530 tiles, ~6,800 cells visited — out of 1M strokes / 2.1M cells |
| Galaxy S24 | **Pending** — APK builds; needs the phone connected with USB debugging |

Numbers are from `og-spike --bench` (chain flight in and out, then a sweep over
the dense 1M-stroke page) and `cargo test --release -p ogpaper-core`.

## Scale test (2026-09-27)

Same flight (chain in/out + dense-page sweep), RX 6600 XT, vsync off.

| Canvas | Frame avg / p99 | Query p99 / max | Max drawn per frame | Build | Peak RAM | GPU data |
| --- | --- | --- | --- | --- | --- | --- |
| 1M strokes | 0.17 / 0.28 ms | 31 / 138 µs | 1,822 strokes | 1.7 s | 0.94 GB | 118 MB |
| 2M | 0.17 / 0.26 ms | 28 / 190 µs | 1,823 | 3.4 s | 1.74 GB | 225 MB |
| 5M | 0.17 / 0.43 ms | 29 / 152 µs | 1,823 | 8.6 s | 3.76 GB | 550 MB |
| 10M | 0.17 / 0.27 ms | 30 / 204 µs | 1,823 | 18.8 s | 7.36 GB | 1.09 GB |
| 5M, packed solid (`--dense`) | 0.18 / 0.41 ms | 81 / 1,372 µs | 23,351 | 6.3 s | 2.53 GB | 550 MB |

**Frame cost does not grow with canvas size** — it depends only on what is on
screen. 1M and 10M strokes render identically.

**What does grow, linearly, is what this spike keeps in memory:**

- RAM ~0.74 GB per 1M strokes: the whole cell tree is resident, ~2.1 cells per
  stroke at ~350 bytes each (a big-integer address per cell, stored twice).
- GPU ~110 MB per 1M strokes: every point is uploaded up front.
- Startup ~1.9 s per 1M strokes (synthetic generation standing in for loading).
- Hard ceiling: one data texture holds 8192 x 16384 = 134M points, ~11.6M strokes.

On a Galaxy S24 (8 GB) or in a browser tab, that caps this spike at a few
million strokes. The renderer is not the limit; residency is.

**The worst case is density, not total count:** a screen packed solid with
small strokes drew 23k strokes in one frame and the query peaked at 1.4 ms —
fine on desktop, tight on a phone at 120 Hz.

### What Phase 1 must add (all already in the design)

1. **Lazy cell loading** from the `.ogp` SQLite file: only cells near the view in
   RAM, evicted LRU. Memory and open time then track the screen, not the canvas.
2. **GPU paging:** upload points per cell on demand into pooled pages, not the
   whole canvas at startup (removes the 134M-point ceiling).
3. **Compact cells:** store addresses relative to the parent (big integers only at
   the top), no duplicate address in the index — target < 64 bytes per cell.
4. **Quantized points** (u16 cell-local, as in the file spec): 4-6 bytes per point.
5. **Density cap:** fewer segments for small strokes (mesh LOD) and real
   thumbnails for cells under ~128 px, bounding per-frame work on packed screens.

## What was built

- `crates/ogpaper-core`
  - `addr` — exact cell addresses `(level, x, y)` with big-integer indices; only
    differences between nearby cells ever become floats.
  - `camera` — camera anchored to a cell with offset in [0,1) and scale in [1,2);
    re-anchors to child/parent when zoom crosses 2×, so its floats stay small.
  - `scene` — sparse quadtree arena; strokes anchored to the cell matching their
    size; per-cell 16×16 occupancy masks for far-zoom tiles.
  - `visible` — per-frame query: exact top cells around the camera, then f64
    descent with culling; content too small to read becomes one occupancy tile
    per 64–128 px region.
  - `gen` — synthetic canvas: a 1M-stroke handwriting page and a 40-step zoom chain.
- `crates/spike` — winit + wgpu app (desktop + Android): GPU-resident points,
  one instanced draw for all visible strokes (SDF capsules, AA), mouse drawing
  with live wet ink, pinch/pan touch, auto-zoom flythrough, `--bench`.

## Run it

```sh
cargo run --release -p og-spike                  # interactive
cargo run --release -p og-spike -- --bench       # prints frame-time stats
cargo run --release -p og-spike -- --start-chain 25   # open at ~10^31 zoom
```

Desktop controls: wheel = zoom, right/middle drag = pan, left drag = draw,
`A` = auto-zoom, `H` = home, `Esc` = quit.

### Android (Galaxy S24)

```sh
cargo install cargo-apk
export ANDROID_HOME=/opt/android-sdk ANDROID_NDK_ROOT=/opt/android-sdk/ndk/27.0.12077973
export CARGO_APK_RELEASE_KEYSTORE=$HOME/.android/debug.keystore CARGO_APK_RELEASE_KEYSTORE_PASSWORD=android
cargo apk build -p og-spike --release --lib
adb -s <S24 serial> install -r target/release/apk/og-spike.apk
adb -s <S24 serial> logcat -s og-spike     # fps / zoom / counts every 0.5 s
```

Touch: one finger pans, two fingers pinch, double-tap toggles auto-zoom. The
1M-stroke canvas is generated at startup (~1.4 s on desktop; expect a few
seconds on the phone). CI also builds the APK as a downloadable artifact.

## Learned

- Plain dots per tiny cell were the bottleneck (91k dots, 7.5 ms query on the
  dense page). Occupancy tiles + skipping cells with nothing in the next three
  levels cut that to ~500 tiles and < 0.1 ms: the design's thumbnail step is needed
  from the start, and a 16×16 bitmask is a cheap first version of it.
- Rule adopted (2026-09-27): content that would be under ~1 px on screen draws
  nothing — no dots or placeholder marks. Tiles only record content that lands
  at >= 1 px when drawn.
- Strokes are anchored to the cell matching their size and may overflow it by
  one cell side; culling on the expanded cell rect handles that.
- Stroke width lives in cell-local units, so the ring around each deeper scene
  becomes a thick band as you zoom through it — the intended Endless Paper feel.

## Next

1. Run the APK on the S24; record fps at the dense page and deep chain.
2. If the phone misses 120 fps: move tile/mesh generation to worker threads and
   cache draw lists between frames when the camera is still.
3. Then close the gate and start Phase 1 (desktop MVP + `.ogp` file).
