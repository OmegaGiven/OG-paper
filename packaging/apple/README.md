# Apple App Store packaging

- `icon-1024.*`: the store and iOS icon (full bleed; the system rounds
  the corners). `icon-mac-1024.*`: the Mac icon (a rounded plate in the
  standard margin). These, the web icons and the Android launcher icons
  all come from `packaging/make-icons.py`.
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
