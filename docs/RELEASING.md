# Releasing OG Paper to the stores

Current state (og-paper task #827, 2026-11-18). The old six-workflow Apple flow is in
[archive/RELEASING-BEFORE-2026-11-18.md](archive/RELEASING-BEFORE-2026-11-18.md).

All store builds run on GitHub Actions from GitHub's copy of the repo
(github.com/OmegaGiven/OG-paper). A change on the forge reaches them only after
`forge publish og-paper` (the owner's step). Nothing here releases to the public by itself:
Apple versions are set to **manual release** and Play uploads go to a **testing track**.

| Store | Workflow | What it does |
| --- | --- | --- |
| App Store (iOS, Mac) | `apple.yml` "Apple release" | build → TestFlight → version + build + What's New → (submit) |
| App Store Connect odds and ends | `asc.yml` "App Store Connect" | status, listing dump, screenshots/text, submit, resubmit, cancel |
| Google Play | `play.yml` "Google Play release" | signed `.aab` → internal / closed / open testing track |
| GitHub release | `release.yml` (tag `v*`) | every platform's files on a GitHub release |
| Private Android mirror | `scripts/publish-android-vault.sh` | APK on the vault (og-host1), not a store |

## Apple: one path

Actions > **Apple release** > Run workflow, or `gh workflow run apple.yml -R OmegaGiven/OG-paper -f ios_version=1.2`:

- `ios_version`, `mac_version`: the version to build for each (empty skips it).
- `whats_new`: file under `packaging/apple/appstore/` (default `whats-new.txt`; edit it first).
  Leave it empty for a platform's first version (Mac 1.0), which has no What's New.
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
   What's New and notes, and sets release to **MANUAL**.
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

Package `app.ogpaper`. `play.yml` inputs: `version` (name shown in the store), `track`
(`internal`, `alpha` = closed testing, `beta` = open testing), `upload`, `draft` (on until the
app has passed its first review), `notes` (file under `packaging/android/`).

It builds the native library with cargo-apk and wraps it in a bundle with
`scripts/android-aab.sh` (aapt2 → bundletool → jarsigner with the upload key; target API 36, as
Play requires since 2026-08-31; manifest in `packaging/android/AndroidManifest.xml`). The version
code is 1000 + the workflow's run number. Then `packaging/android/play.py` uploads it with the
service account and puts it on the track. The `.aab` is kept as artifact `og-paper-aab` either way.
Production is never set by the workflow: promote a tested release in Play Console.

### One-time steps only the owner can do (Play Console)

1. **Developer account**: play.google.com/console, identity verification done.
2. **Create app**: name OG Paper, default language English (US), App, Free; accept the
   declarations. The package name is fixed by the first upload: `app.ogpaper`.
3. **Upload key** (if `ANDROID_KEYSTORE_B64` is not set yet, see secrets below), then run
   `play.yml` with `upload` **off**, download the `og-paper-aab` artifact, and upload it **by
   hand** once: Testing > Internal testing > Create release. Accept **Play App Signing** (Google
   keeps the app signing key; ours is only the upload key). Play's API refuses a new app's first
   bundle, so this first one is manual; later ones go through the workflow.
4. **Store listing**: short and full description (`packaging/apple/appstore/description.txt` is a
   start; no mention of other platforms' stores), app icon 512×512
   (`packaging/apple/icon-1024.png` scaled), feature graphic 1024×500, at least 2 phone
   screenshots (and 7"/10" tablet ones for tablet listing), category Art & Design, contact email,
   privacy policy URL `https://omegagiven.github.io/OG-paper/privacy/`.
5. **App content**: privacy policy; ads = none; app access = all features without sign-in (say
   that sign-in is optional; give the review test account if asked); **content rating**
   questionnaire (IARC: drawing app; users can share pages and chat on hosted pages = user
   interaction / user-generated content); target audience (13+ suits the chat); news app = no;
   **data safety**: what the optional OG Studio account collects (email, name, pages the user
   keeps there, chat messages), encrypted in transit, users can ask for deletion; the
   **account deletion URL** (ogstudio.org account page where deletion is confirmed), required
   because the app lets people make an account.
6. **Service account** for the workflow: in Google Cloud (any project) enable the *Google Play
   Android Developer API*, create a service account, create a **JSON key** for it. In Play
   Console > Users and permissions > Invite new users: the service account's email, app OG Paper,
   permissions *Release to testing tracks* and *View app information*. (Production rights only if
   you want the workflow to promote later; it doesn't now.)
7. **Testers**: Internal testing > Testers: an email list (up to 100). New personal developer
   accounts (made after 2023-11-13) must run a **closed test** (track `alpha`) with **at least
   12 testers opted in for 14 days in a row** before Play lets them apply for production access.
   Make a closed-testing track with a Google Group or email list of 12+ people, share the opt-in
   link, and keep them in for the 14 days.
8. After the closed test: Publishing overview > **Apply for production**, answer the questions,
   then promote the tested release to Production yourself.

### Play secrets (to add on GitHub)

| Secret | Status 2026-11-18 | What |
| --- | --- | --- |
| `ANDROID_KEYSTORE_B64` | **missing** | the upload keystore, base64 |
| `ANDROID_KEYSTORE_PASSWORD` | **missing** | its password (also the key's unless `ANDROID_KEY_PASSWORD`) |
| `ANDROID_KEY_ALIAS` | optional | key alias (default: the keystore's only key) |
| `ANDROID_KEY_PASSWORD` | optional | key password, if it differs from the keystore's |
| `PLAY_SERVICE_ACCOUNT_JSON` | **missing** | the service account's JSON key file, as is |

Making and adding them (on your own machine; the password is typed into prompts, never stored in
the repo or a chat):

```sh
keytool -genkeypair -keystore og-paper-upload.jks -alias upload -keyalg RSA -keysize 4096 -validity 20000 -dname "CN=OG Paper,O=OG Studio"
base64 -w0 og-paper-upload.jks | gh secret set ANDROID_KEYSTORE_B64 -R OmegaGiven/OG-paper
gh secret set ANDROID_KEYSTORE_PASSWORD -R OmegaGiven/OG-paper        # prompts for it
gh secret set PLAY_SERVICE_ACCOUNT_JSON -R OmegaGiven/OG-paper < play-service-account.json
```

Keep `og-paper-upload.jks` and its password somewhere safe (password manager). If the upload key
is lost, Play can reset it (Play App Signing), so it isn't fatal. `ci.yml` signs its APKs with
the same key once it is set; the private vault mirror keeps its own key on og-host2, so an APK
from one source must be uninstalled once before installing from the other. A Play install
(signed by Google) is a third signer.

Check what Play has: `python3 packaging/android/play.py status` with
`PLAY_SERVICE_ACCOUNT_JSON` set, or the workflow's last step.
