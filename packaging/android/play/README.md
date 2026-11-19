# Google Play listing art

What `scripts/play-listing.sh` uploads (en-US):

| Path | What |
| --- | --- |
| `listing/title.txt`, `short-description.txt`, `full-description.txt` | store text |
| `listing/whats-new.txt` | release notes for the next release (`scripts/play-release.sh --notes`) |
| `icon-512.png`, `feature-graphic.png` (1024 x 500) | `make-art.py`, from the app icon |
| `phone/` (1080 x 2160), `tab7/` (1200 x 1800), `tab10/` (1600 x 2440) | screenshots of the real Android app |

## Screenshots (task #884)

Taken in the Android app itself, in the emulator (`/opt/android-sdk`, system image
android-34 google_apis x86_64, which runs the arm64 app), one AVD per size:
phone 1080 x 2160 at 400 dpi, 7" 1200 x 1920 and 10" 1600 x 2560 at 320 dpi. Tablets lose
their taskbar (120 px, it shows other apps' icons) in the crop.

1. **Canvases.** The native apps have no try mode, so the demo is written out as files:
   `OG_STORE_OUT=out/storecanv/phone OG_STORE_SIZE=1080,2160 cargo test --release -p og-paper
   --lib store_canvases -- --ignored` gives `street.ogp` and `deep.ogp` framed for that screen.
   The other scenes are the App Store ones (`tests/ui/store-scenes.mjs`) built on a page
   server: `og-paper --serve-dir DIR --port 8995`, then
   `node scripts/ad/play-canvases.mjs ws://127.0.0.1:8995/api "$(cat DIR/server.key)" W H`
   with W x H the screen in **physical pixels** (the native home view is 1 point per pixel).
   Checkpoint each page (`sqlite3 X.ogp 'PRAGMA wal_checkpoint(TRUNCATE)'`) before copying it.
2. **Into the app.** `adb root`, push the five `.ogp` files to
   `/data/data/app.ogpaper/files/canvases/` (chown to the app's uid, `restorecon`), with
   mtimes street, deep, diagrams, design, sketch (newest first: Pages lists by date).
   Status bar: SystemUI demo mode (clock 9:30, full battery).
3. **Capture.** For each: fresh app data, Settings > Pages > the canvas > Open, close the
   panel, `adb exec-out screencap -p`. Sketch: tap the toolbar switcher so the toolbars and
   inventory show.
4. **Compose.** `python3 scripts/ad/play-shots.py RAWDIR` crops and adds the captions.

The older web-rendered set (`tests/ui/features/store-play.mjs`) still works for a quick
refresh, but its UI is the browser's, not the app's.
