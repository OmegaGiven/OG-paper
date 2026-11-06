# Sign-in on the phone and desktop apps: plan

2026-11-06. Plan only; nothing here is built. Goal: an optional prompt in the
native apps to sign in through the website, so pages are kept on the Cloudflare
server and can be recovered, with a copy kept on the device as well.

## Where things stand

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

Use the same OAuth server the AI connector needs (`docs/AI-CONNECTOR.md`,
section 9), so it is built once. The native app is a public client with PKCE.

1. App shows the prompt, and opens the system browser at
   `ogstudio.org/paper/api/oauth/authorize`.
2. The website signs the person in as it does today, with Google, GitHub or
   Apple. Passwords and Google tokens never reach the app.
3. The browser returns to the app: `ogpaper://` on iOS and Mac (ASWebAuthentication
   Session on iOS), loopback on Windows and Linux. Android needs its own check.
4. App swaps the code for a device token: Keychain on Apple, the OS store where
   there is one. Each device shows in My account and can be revoked.
5. API and relay accept `Authorization: Bearer`. The current check refuses
   non-GET requests with no Origin; bearer requests are exempt since they carry
   no cookie.
6. The Pages panel gains My account: list, open, keep this page. Opening keeps a
   copy on the device, as server pages already do (`link.<canvas>`).

Needs an HTTP client in the native build; today it has only WebSockets.

## Work

| Step | Where | Size |
| --- | --- | --- |
| OAuth server, device tokens, bearer on API and relay, tests | `hosted/` | M, 3-5 days |
| Sign in with Apple; delete account | omegagiven-com, as its own task | M, 3-5 days |
| Sign-in flow, token storage, URL scheme, account panel, prompt | Rust app | L, 2 weeks |
| Apple privacy labels, notes, version 1.1 | App Store Connect | S, plus review |

No Mac is on the tailnet: Mac and iOS sign-in can be tested only in CI and
TestFlight, so a person has to try the real flow on a device.
