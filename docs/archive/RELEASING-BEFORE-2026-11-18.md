# Store releases before one Apple path and Play (archived 2026-11-18)

Superseded by [../RELEASING.md](../RELEASING.md) (og-paper task #827).

Until then the Apple release was six hand-run GitHub workflows, run one after another:

- `ios.yml` "iOS App Store": Simulator launch test, then build, sign, validate and upload an
  `.ipa`. Input: version. Build number = that workflow's run number (iOS reached 12).
- `appstore.yml` "Mac App Store": universal build, sandboxed launch test
  (`tests/launch/mac-launch.sh`), sign, `.pkg`, validate, upload. Build number = its own run
  number (Mac reached 7), so the two workflows' numbers were unrelated.
- `asc-listing.yml`: make or renumber the version (`create`), attach the newest processed build
  (`attach_build`, only once Apple had processed it, so a second run minutes later), set
  release type, What's New, notes, description, copyright, screenshots, previews.
- `asc-submit.yml`: submit, `fresh` (cancel a rejected submission; it then failed on "already
  added", wait ~2 min, run plain submit again), `cancel`.
- `asc-status.yml`, `asc-review.yml`: read-only status and full listing dump.

The order for an update was: ios.yml version=X → asc-listing create=true release=MANUAL
attach_build=true whats_new=… → asc-status (check the build number) → asc-submit.
Android had no store: `ci.yml` built an APK (signed with `ANDROID_KEYSTORE_B64` when set) for
GitHub releases, and `scripts/publish-android-vault.sh` put one on the private vault mirror.
