# Sign-in on the phone and desktop apps

2026-11-06. An optional prompt in the native apps to sign in through
ogstudio.org, so pages are kept on the Cloudflare server and can be recovered,
with a copy kept on the device as well. Ships in the next version.

## Status

Built on branch `worktree-task-native-signin`, merged into main with vault mode (task #792, [VAULT.md](VAULT.md)); not released. The sign-in now offers OG Studio (this flow) or a vault.

| Part | State |
| --- | --- |
| Server: sign-in, device tokens, delete account (`hosted/src/oauth.js`) | Done, tested on real workerd |
| App: prompt, sign-in, account pages, keep, recover (`account.rs`) | Done on Linux; a page came back on an empty second device |
| Web: first sign-in keeps the open canvas | Done, browser test passes |
| Privacy page | Updated on the branch, published with the release |
| Sign in with Apple, delete-account page | omegagiven-com task #355 |
| Mac and iPhone | Compile and Keychain **not yet tested**: need Apple's toolchain, so GitHub's Mac runners |
| Android | Builds with TLS and INTERNET since #792; OG Studio sign-in needs ogstudio.org to deploy `oauth.js` first |

Tests: `forge run --on og-host1 bash tests/account/run-e2e.sh`;
screenshots: `tests/account/shots.sh`.

## Before this work

| | Web app | Native apps |
| --- | --- | --- |
| Sign in with Google or GitHub | Yes | **No** |
| Pages on the server | Yes | **No** (only self-hosted page servers, with a password) |
| Copy on the device too | Yes (IndexedDB, reopens offline, changes go up on reconnect) | Local files only |
| Prompt to sign in | A status line and the My account panel, no first-run prompt | **No** |

Two behaviours on the web to decide on:
- Browser pages are not uploaded by themselves: **Keep this one** is per page.
- Signing out removes account pages from the device. They come back on sign-in.

## What blocks a native release

1. **Sign in with Apple (guideline 4.8).** Google or GitHub as the primary
   account requires an equivalent privacy-friendly login. "Your company's own
   account setup" is an exception, but the site has no sign-in of its own, so
   it does not apply. Needs a provider in omegagiven-com, and a Services ID and
   key from the owner's Apple Developer account.
2. **Account deletion in the app (5.1.1(v)).** The site has none: `auth.js` only
   deletes sessions. Needs a delete flow covering pages, members, rooms, offers
   and sessions, reachable from the app.
3. **Sign-in must stay optional (5.1.1(v)).** The prompt must be dismissible,
   with drawing unaffected.
4. **Store paperwork.** App Privacy labels change (no longer "no data
   collected"), and the review notes' "No account or sign-in is needed" needs
   rewording, with a reviewer account. Ships as a new version, not in Mac build 7.

## Design

Used a polling flow, not the redirect first planned. An iPhone suspends the app
while Safari is in front, so nothing could listen for a redirect, and the usual
fix (`ASWebAuthenticationSession`) is native code that can't be run without a
Mac. Polling works the same everywhere and could be tested here.

1. The app makes a PKCE pair and opens the system browser at
   `ogstudio.org/paper/api/oauth/authorize`.
2. The website signs the person in as it does today. Passwords and Google,
   GitHub or Apple tokens never reach the app.
3. The page asks them to agree, and shows a six-character code the app shows
   too, so nobody can get them to approve a sign-in that isn't theirs.
4. Meanwhile the app asks `/oauth/token` every 3 seconds with the verifier only
   it holds, and collects its tokens once they agree. `ogpaper://open` on the
   last page brings the app back; it needs no handling, only the URL type.
5. Tokens stay in the Keychain on Apple systems, and otherwise in a file only
   the user can read. API and relay take `Authorization: Bearer`. Admin rights
   never travel with a token.
6. Pages lists the account's pages. Opening one joins its relay link, so the
   device keeps a copy as usual. First sign-in keeps the open page by itself.

Not done: other pages on the device go up one at a time with **Keep this page**.

## To release

1. Finish #355, and create the Apple Services ID and key (owner).
2. ~~Merge this branch~~ (merged with #792); omegagiven-com bumps `vendor/og-paper` and deploys (until then OG Studio sign-in in the apps answers 404).
3. Try the real sign-in on ogstudio.org.
4. `forge publish`, then build version 1.1 for Mac and iOS on GitHub's runners.
5. Try it on a real iPhone and Mac through TestFlight.
6. Update App Privacy in App Store Connect (below), set the 1.1 review notes
   (`packaging/apple/appstore/review-notes-1.1.txt`), and submit.

App Privacy changes: Contact Info (name, email) and User Content (pages), linked
to the user, used for app functionality, not for tracking. Only for people who
sign in.

No Mac is on the tailnet: Mac and iOS sign-in can be tested only in CI and
TestFlight, so a person has to try the real flow on a device.
