# Apple App Store packaging

- `icon-ios-1024.png`: the iOS app icon, the og-design kit's `paper` tile, symbol only
  (kit 0.20.1 `tiles/png/paper-ios-1024.png`). `icon-1024.*`: the store art source (Play icon and
  feature graphic). `icon-mac-1024.*`: the Mac icon (a rounded plate in the standard margin).
  `icon-1024.*`, `icon-mac-1024.*` and `packaging/icon.svg` come from `packaging/make-icons.py`;
  the web (PWA, apple-touch, favicon) and Android launcher icons are the kit's, vendored with
  `tools/vendor.sh tile paper web/app` and `launcher paper crates/og-paper/res`.
- `macos/OGPaper.entitlements`: the Mac App Store sandbox: files the user
  picks, network client (joining) and server (hosting). Canvases, fonts,
  plugins and settings live in the app's container.
- `macos/Info.plist.in`: the bundle's Info.plist (`__VERSION__` and
  `__BUILD__` are filled in by the workflow).

## Builds and releases

`.github/workflows/apple.yml` builds, tests, signs and uploads both apps and prepares the
version in App Store Connect; `asc.yml` does the rest (status, listing, submit). How to run
them and the secrets they need: [docs/RELEASING.md](../../docs/RELEASING.md).

- `ios/assemble.sh`: the iOS app bundle (Info.plist, icon); signing is in the workflow.
- `asc/`: App Store Connect API scripts (`status.py`, `next_build.py`, `listing.py`,
  `submit.py`, `review.py`).
- `appstore/`: store text (descriptions, What's New, review notes) and screenshots.

Bundle ID `com.omegagiven.ogpaper`, team `FGFW7ZAJUC`.
