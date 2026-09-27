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
- Strokes are anchored to the cell matching their size and may overflow it by
  one cell side; culling on the expanded cell rect handles that.
- Stroke width lives in cell-local units, so the ring around each deeper scene
  becomes a thick band as you zoom through it — the intended Endless Paper feel.

## Next

1. Run the APK on the S24; record fps at the dense page and deep chain.
2. If the phone misses 120 fps: move tile/mesh generation to worker threads and
   cache draw lists between frames when the camera is still.
3. Then close the gate and start Phase 1 (desktop MVP + `.ogp` file).
