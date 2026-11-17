# OG Paper on an OG Vault (vault mode)

Current state, 2026-11-16 (og-paper task #792, vault side og-vault #791). The previous
sign-in (OG Studio only) is in [archive/SIGN-IN-BEFORE-VAULT.md](archive/SIGN-IN-BEFORE-VAULT.md).

OG Paper keeps pages in one of two kinds of account, chosen when signing in:

| | OG Studio | Vault |
| --- | --- | --- |
| Server | ogstudio.org (`hosted/`, Cloudflare) | an OG Vault (`vaultd`, og-vault `docs/PAPER.md`) |
| API | `/paper/api` | `/api/v1/paper` |
| Web sign-in | the site's cookie session | vault app sign-in, PKCE (og-vault `docs/APP-LOGIN.md`), bearer tokens |
| Native sign-in | poll flow with a pair code ([NATIVE-SIGN-IN.md](NATIVE-SIGN-IN.md)) | PKCE, the browser comes back to a one-time `http://127.0.0.1:<port>/` listener |
| Live room | hosted `PaperRoom`, cookie or bearer | `vaultd` relay, one-use `ticket` (browser) or bearer (apps) |
| Sharing | people by username/email, links, co-owners, offers | vault people (and teams), viewer/editor |

Rooms speak the same relay protocol either way ([HOSTED.md](HOSTED.md) "Live protocol"), so
drawing, offline copies and merging are the same code. Existing OG Studio users see one change:
the sign-in now has two buttons, **OG Studio** and **Vault**.

## Signing in

1. Account panel (web: *My account*; native: Pages → Account, or the first-run card) offers
   **OG Studio** and **Vault**.
2. Vault: an address field (filled with this vault when the web app is served by one, `<meta
   name="og-paper-vault" content="self">`), a QR button where the browser can scan
   (`BarcodeDetector`), and Continue. Connect links (`ogvault://connect?u=&id=&n=`) and bare
   addresses both work. Native apps take a typed or pasted address/link (no camera yet).
3. The app probes `<address>/.well-known/og-vault` (`server: og-vault`, `vault_id`), makes a PKCE
   pair and opens `/auth/vault/start?client_id=og-paper&redirect_uri=…&code_challenge=…&state=…`.
   - web: `redirect_uri` = this page (`<vault>/paper/`, or any page on 127.0.0.1); the vault's
     page sends the browser back with `?code&state`; `finishLogin` swaps it at `/auth/token`.
   - native (desktop, Android, iOS): `redirect_uri` = `http://127.0.0.1:<free port>/`, a listener
     the app runs until the browser comes back (10 min), then swaps the code.
4. The vault is probed again (same `vault_id`), the profile saved and made active.

The vault registers `og-paper` with redirects `ogpaper://auth`, `/paper/` and
`http://127.0.0.1/` on any port (og-vault `app_clients.rs`).

## Profiles (og-vault PROFILES.md)

- One profile per vault (`vault_id`), one in use at a time. Web: `og-paper:og:v1:profiles` in
  localStorage, tokens AES-GCM encrypted with a non-extractable WebCrypto key
  (`web/app/vault/og-profiles.js`, a copy of og-vault `clients/og-profiles`; keep in step).
  Native: `vaults.json` next to the app's data, tokens in the Keychain (Apple) or a 0600 file
  (`vault-<id>.txt`; on Android the app's private storage).
- Which account is in use: web `og-paper-backend` (`studio` | `vault:<id>`), native pref
  `account.backend`.
- Web local state of a vault profile is keyed `og:v1:<vault_id>:<person>:<name>` (page links,
  sync marks, last page, owner); OG Studio keeps its old keys, so nothing changes for it.
- **Switch** in the account panel lists OG Studio (where the site has it / where signed in) and
  every vault profile (`user @ vault`, a dot for reachable on the web); tapping one switches
  (web: saves, then reloads as that account). Sign out removes that vault profile, its tokens and
  its local keys; its pages leave the browser as on OG Studio (asked first if unsynced).
- On start the saved addresses are probed; tokens go only to an address that answered with the
  profile's `vault_id`. Not reachable: the last page opens from this device's copy and joins when
  the vault answers (web: on `online`).
- Relay links from the vault name its public address; the client rewrites them to the address
  that answered (LAN, tailnet), web `VaultProvider.here`, native `link_of`.

## Code

| Where | What |
| --- | --- |
| `web/app/og-vault.js` | profiles store, `VaultSession` (probe, refresh, bearer fetch), `signInAt` / `finishVaultSignIn`, `VaultProvider` (HostedProvider over `/api/v1/paper`: tickets, link rewrite, vault people for sharing; no links/offers/chat/history/reports) |
| `web/app/og-hosted.js` | the account panel for both kinds: `startHosted(ui, cfg, vault)`, `signInChoice`, `accounts()` (Switch), keyed local state (`setKeys`) |
| `web/app/og-web.js` | finishes a vault sign-in on load, picks the session, dials rooms through `hosted.dialUrl` (async ticket) |
| `web/app/boot.js` | the app's start (was inline in index.html; the vault's CSP has no `unsafe-inline` scripts) |
| `crates/og-paper/src/account.rs` | native accounts: `Backend::{Studio, Vault}`, vault probe, PKCE + loopback (`vault_sign_in`, `wait_for_vault`), refresh, profiles, `bearer_for` |
| `crates/og-paper/src/ui.rs` | `account_section`: OG Studio / Vault buttons, vault address field, Switch list |

Android now builds with TLS (rustls) and the INTERNET permission, so wss rooms and the vault work
there.

## Tests

- `tests/vault/run-e2e.sh`: the web app in Chromium against a throwaway vault (sign-in through
  the vault page, new page, second device, live both ways, offline edits merging, reload, Switch,
  sharing with another person). Start the vault first (og-vault `deploy/homelab/deploy.sh` with
  `OGV_APP=og-vault-e2e-paper792 OGV_PORT=18792`).
- `tests/vault/run-native.sh`: the native app's vault sign-in (loopback), keep, a second device
  and live drawing, same throwaway vault.
- `cargo test -p og-paper --lib account`: addresses, connect links, start URL, token expiry, the
  loopback listener.
- `tests/vault/perf-ab.mjs`: frame times while panning, old build vs new (16.7 ms median both,
  vsync-bound in headless Chromium, 2026-11-16).
- OG Studio is unchanged: `hosted/test/run-browser-e2e.sh` (all four suites) and
  `node --test hosted/test/*.test.js`.

## Deploy

- Web, vault test path only: `scripts/deploy-vault-web.sh` → og-host1
  `/var/lib/og-host/apps/og-vault/dist/og-paper/web`, served at
  `https://og-host1.tail6c6721.ts.net:8610/paper/`. The public app (ogstudio.org, `web-dist`) is
  not touched.
- Android, private mirror only: `scripts/publish-android-vault.sh "notes"` builds on og-host2
  (key `/cache/android/keys/og-paper.keystore`, made on first use), publishes to og-host1
  `dist/og-paper/{og-paper.apk,release.json}`, served at `/api/v1/apps/paper/android(.apk)` and on
  the vault's Home/Connect. Versions `0.<2+n/250>.<n%250>` from the commit count, so each build
  installs over the last. An APK from GitHub's CI has another key: uninstall it once.
- Desktop and iOS: built from the same code, released later together after testing (iOS's
  loopback return is untested: iOS may suspend the app while Safari is in front; the vault already
  accepts `ogpaper://auth` for an `ASWebAuthenticationSession` return if needed).

## Not done yet

- QR scanning in the native apps (paste the connect link, or type the address).
- Automatic switching between profiles on network change (manual Switch only); the web probes on
  start and on `online`.
- Vault sharing with teams from the app (people only; teams work from the vault's own UI).
