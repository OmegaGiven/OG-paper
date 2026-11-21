# Releasing OG Paper to the stores

Current state (og-paper task #827, 2026-11-18). The old six-workflow Apple flow is in
[archive/RELEASING-BEFORE-2026-11-18.md](archive/RELEASING-BEFORE-2026-11-18.md).

Apple store builds run on GitHub Actions from GitHub's copy of the repo
(github.com/OmegaGiven/OG-paper). A change on the forge reaches them only after
`forge publish og-paper` (the owner's step). Nothing here releases to the public by itself:
Apple versions are set to **manual release** and Play uploads go to a **testing track**.
Google Play releases run from the forge itself (`scripts/play-release.sh`), no publish needed.

| Store | Workflow | What it does |
| --- | --- | --- |
| App Store (iOS, Mac) | `apple.yml` "Apple release" | build → TestFlight → version + build + What's New → (submit) |
| App Store Connect odds and ends | `asc.yml` "App Store Connect" | status, listing dump, screenshots/text, submit, resubmit, cancel |
| Google Play | `scripts/play-release.sh` (forge runner + forge store key); `play.yml` build-only | signed `.aab` → internal / closed / open testing track |
| GitHub release | `release.yml` (tag `v*`) | every platform's files on a GitHub release |
| Private Android mirror | `scripts/publish-android-vault.sh` | APK on the vault (og-host1), not a store |

## Apple: one path

Actions > **Apple release** > Run workflow, or `gh workflow run apple.yml -R OmegaGiven/OG-paper -f ios_version=1.2`:

- `ios_version`, `mac_version`: the version to build for each (empty skips it).
- `whats_new`: file under `packaging/apple/appstore/` (default `whats-new.txt`; edit it first).
  A platform's first version has no What's New; `listing.py` skips it there (Mac until 1.2 is live).
  Note: `gh workflow run -f whats_new=` sends the default, not empty.
- `notes`: App Review notes file (empty leaves them).
- `submit`: off by default. On, it sends the version to App Review after the build is attached.

What it runs, in order:

1. **number**: next build number = App Store Connect's highest build + 1 (`asc/next_build.py`),
   the same for both platforms, so numbers never go back whichever workflow made the last one.
2. **ios**: Simulator launch test (artifact `ios-simulator`), then build for iPhone and iPad,
   sign, `.ipa`, validate, upload. **mac**: universal build, sandboxed first-launch test
   (artifact `mac-launch-test`), sign, `.pkg`, validate, upload. Both land in TestFlight.
3. **App Store Connect**: makes the version if App Store Connect has none (or renumbers the one
   in preparation), waits for Apple to process the build (up to 90 minutes), attaches it, sets
   What's New and notes, and sets release to **AFTER_APPROVAL** (live as soon as Apple approves).
4. With `submit` on: submits for review. After approval nobody but the owner presses
   **Release** in App Store Connect.

After a rejection: fix, publish, run Apple release again with the same version (a new build
replaces the old), then App Store Connect `action=resubmit`. If it says "already added", wait two
minutes and run `action=submit`.

**App Store Connect** (`asc.yml`), `action`: `status` (read only, the default), `review` (whole
listing + screenshots as an artifact), `listing` (`shots`/`previews` folders; `files` like
`description=description-ios.txt whats_new=whats-new.txt copyright=2026_OG_Studio`, `_` for
spaces), `submit`, `resubmit`, `cancel`.

### Apple secrets (all set on GitHub as of 2026-11-18)

| Secret | What |
| --- | --- |
| `APPLE_API_KEY`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER` | App Store Connect API key (.p8 text), its ID, issuer ID |
| `APPLE_DIST_CERTIFICATE`, `APPLE_DIST_CERTIFICATE_PASSWORD` | Apple Distribution certificate (.p12, base64) and its export password |
| `APPLE_DIST_SIGNING_IDENTITY` | `Apple Distribution: <account holder> (FGFW7ZAJUC)` |
| `APPLE_INSTALLER_CERTIFICATE`, `APPLE_INSTALLER_CERTIFICATE_PASSWORD` | Mac Installer Distribution certificate (.p12, base64) and password |
| `APPLE_INSTALLER_SIGNING_IDENTITY` | `3rd Party Mac Developer Installer: <account holder> (FGFW7ZAJUC)` |
| `APPLE_IOS_PROVISIONING_PROFILE`, `APPLE_MAC_PROVISIONING_PROFILE` | App Store profiles (base64) |

`.p12` files made with OpenSSL 3 need `openssl pkcs12 -export -legacy`. Certificates and profiles
expire after a year (October 2027): renew them in the Apple Developer site and replace the secrets.

## Google Play

Package `app.ogpaper`, Play Console app id 4974630082929640996 (account 7847877454503027168).
Set up by task #844 on 2026-11-18; the earlier plan (GitHub secrets, first bundle by hand) is in
[archive/RELEASING-PLAY-BEFORE-844.md](archive/RELEASING-PLAY-BEFORE-844.md).

**State (2026-11-19, task #884):** closed testing (alpha) has **1.2 (1002)** (the menu-tap fix
plus the Android status-bar fix), and the whole setup (closed test, listing with screenshots from
the Android app, app content: content rating IARC Everyone / PEGI 3, target audience 13+, ads none,
Data safety, privacy policy https://ogstudio.org/paper/privacy/, app access "all functionality
without special access", category Art & Design) passed Play's quick checks and is **in review**
(the API commit sends changes for review, as managed publishing is off). Play App Signing is on
with a Google-generated app signing key; our key is only the upload key. Internal testing has
1.0 (1001). History: task #847 set it up, #884 sent it.

### Releasing a build (the forge path, no key on any laptop or on GitHub)

```sh
scripts/play-release.sh --version 1.1                 # internal testing, live for its testers
scripts/play-release.sh --version 1.1 --track alpha   # closed testing (alpha) / beta = open testing
scripts/play-release.sh --version 1.1 --draft         # leave the release as a draft in Play Console
```

1. Version code = highest bundle Play has + 1 (at least 1000).
2. Builds and signs the `.aab` on og-host2's forge runner (`scripts/android-aab.sh`: cargo-apk →
   aapt2 → bundletool → jarsigner; target API 36). The upload key is
   `/cache/android/keys/og-paper-upload.keystore` (+ `.pass`) on og-host2, root-only, never
   copied off the runner.
3. Uploads through the forge's store API with credential `play-main` (service account
   `og-forge@omegagiven.iam.gserviceaccount.com`, key sealed in the forge vault; Play
   permissions: release to production and testing, manage testing tracks), puts it on the track
   and commits the edit.

Production is never set by the script: promote a tested release in Play Console. Any agent with
the forge token for og-paper can run it. The script calls the forge HTTP API directly because the
installed `forge store call` (0.2.0) answers 404.

If the upload key is lost (og-host2 cache wiped), Play Console > App signing > Upload key >
*Request reset* with a new key; the app signing key is Google's, so installs keep updating.

The private vault mirror (`scripts/publish-android-vault.sh`) signs with its own key
(`og-paper.keystore` on og-host2). A Play install is signed by Google's key, so switching a
device between Play and the mirror needs one uninstall.

### Settings that must stay

- **Protected with Play > Automatic protection > Installer check: off.** Anti-tamper needs dex
  code; OG Paper is a NativeActivity with none, and uploads fail with "Anti-tamper protection is
  not available for this app as it has no dex files".

### `play.yml` (GitHub Actions)

Left in place but idle: it signs with `ANDROID_KEYSTORE_*` and uploads with
`PLAY_SERVICE_ACCOUNT_JSON` from GitHub secrets, which are deliberately **not** set (both keys
stay on the forge), so it stops at "secret ANDROID_KEYSTORE_B64 is missing". Release with
`scripts/play-release.sh`; `scripts/play-release.sh --build-only` gives a signed `.aab` locally.

### Still the owner's (Play Console)

1. **Privacy page live**: `https://ogstudio.org/paper/privacy/` still answered 404 when 1.2 went
   to review (task #883's deploy of omegagiven-com was blocked). Deploy it (omegagiven-com
   `cde2d8c`, `public/paper` at web-dist `80ca10a`) before the reviewer opens it, or the review
   can fail on the privacy policy / account deletion URLs.
2. **Testers**: a personal account needs a closed test with **12+ testers opted in for 14
   days** before Production. Add their Gmail addresses to the "Fulfillment testers" list
   (Closed testing - Alpha > Testers; the forge browser can't type into Play's email-chip
   field), Save, and send them the opt-in link from that tab once the review passes.
3. **Trader status** (banner in Play Console): declare trader / non-trader for the EU.
4. **Production**: after 14 days with 12+ testers: Dashboard > *Apply for production*, answer the
   closed-test questions, then promote the tested release yourself.
