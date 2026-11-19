# Security pass: OG Paper (task #997, 2026-11-19)

Scope: the apps as they ship now (web, vault mode, Android APK/AAB, desktop, iOS/Mac build),
auth/tokens, file and media handling, WebView, dependencies. The hosted server (`hosted/`) was
audited in [LAUNCH-AUDIT.md](LAUNCH-AUDIT.md) (tasks #178, #219, 2026-11); that list is the
base and its status table is still the record for H1-H5, M1-M4, L1, L3.

## What was checked

| Area | Result |
| --- | --- |
| Android | Native activity only (`hasCode=false`): **no WebView, no Java**, no JS bridge, one permission (`INTERNET`), cleartext off (targetSdk 34), one exported activity (launcher only, no custom intent filters, so no deep-link input) |
| Tokens (native) | OG Studio and vault tokens in the Keychain (Apple) or a `0600` file in the app's private dir; sign-in is PKCE with `state`, loopback redirect, no password seen. Tokens only go to an address that answered as that vault's id |
| Tokens (web) | Vault tokens encrypted with a non-extractable WebCrypto key; bearer sent with `credentials: 'omit'` (no cookie, no CSRF); keys prefixed per app |
| Media / files | Pictures and PDFs are sealed in changes and only drawn to a canvas, never served or rendered as HTML; page text goes through `esc()` before `innerHTML`; `.ogp` is SQLite opened with bound parameters |
| Dependencies | `cargo audit` (RustSec, 2026-11-19): 7 vulnerabilities, all fixed below. No npm deps in `web/` or `hosted/`. tokio and rustix soundness warnings fixed by update; 5 "unmaintained" and 3 "yanked" warnings remain (see Open) |
| Web supply chain | pdf.js was loaded from cdnjs at runtime; now our own copy |

## Findings and status

| # | Sev | Finding | Status |
| --- | --- | --- | --- |
| S1 | **High** | `rustls` 0.23.45: ECH-rejection data leak, TLS secrets in `Debug`, ServerHello downgrade (RUSTSEC-2026-0346/0347/0348/0349); `rustls-webpki` 0.103.15 accepted distrusted intermediates (0382); `webpki-roots` 1.0.9 trusted distrusted emSign intermediates (0381) | **Fixed**: rustls 0.23.46, rustls-webpki 0.103.17, webpki-roots 1.0.12 |
| S2 | **Medium** | `libsqlite3-sys` 0.38 bundled SQLite with CVE-2026-119375 (RUSTSEC-2026-0459); `.ogp` files and shared pages are SQLite | **Fixed**: rusqlite 0.43 (bundled SQLite updated) |
| S3 | **Medium** | Sign-in accepted `http://` to *any* host, so tokens and the PKCE code could cross the open internet in clear (typo or a hostile connect link) | **Fixed**: http only for loopback, private, link-local, tailnet (100.64/10) addresses and bare/`.local` names; native (`account.rs`) and web (`og-vault.js`); tests added |
| S4 | **Medium** | Android auto-backup was on by default: the private token file could leave in a Google backup or device transfer | **Fixed**: `allowBackup=false` (APK metadata and the AAB manifest) |
| S5 | **Medium** (M8) | pdf.js from cdnjs at runtime, no integrity check; a CSP has to allow all of cdnjs; PDF import broke under the vault's strict CSP | **Fixed**: pdf.js 4.10.38 vendored in `web/app/vendor/pdfjs/`, `isEvalSupported: false`. cdnjs can leave every CSP |
| S6 | Info | Beta badge added (kit 0.15.0 look, one switch `BETA` in `ui.rs`) | done, not a security item |

Earlier items (LAUNCH-AUDIT) already fixed: H1, H2, H4 (trash), H5, M1-M4, L1, L3.

## Open: needs the owner, or not code

| Item | Sev | Why it isn't code |
| --- | --- | --- |
| C1 Workers Free plan | Critical (hosted only) | Billing: move omegagiven.com/ogstudio.org to Workers Paid. Does not affect vault mode or the APK |
| C2 Terms/Privacy v2, abuse and DMCA contact | Critical before paid | Legal text and a real contact |
| H3 shared origin with the rest of omegagiven.com | High (hosted only) | Needs a `paper.` subdomain: DNS/Worker routes in omegagiven-com. Vault mode serves OG Paper on its own origin already |
| H4 backups, H6 admin page, H7 paid storage, M6 monitoring, L2, L5-L9 | High/Med/Low | Ops, billing and site work in omegagiven-com or og-host1; none touches the apps you test |
| M9 relay key in the socket URL (`?k=`) | Medium (hosted only) | The key *is* the link's capability in the relay protocol; changing it changes the wire format for every client. Cloudflare Worker logs keep query strings: turn `observability` off or redact in omegagiven-com. Vault tickets (`?ticket=`) are one-use |
| M10 "end-to-end" wording | Medium | Copy only: do not market hosted pages as E2E (the site holds keys) |
| Unmaintained/yanked crates (`chrono`, `ttf-parser`, `zbus_names`, `zvariant*`; yanked `async-executor`, `clipboard-win`, `yoke-derive`) | Low | No fixed version exists; upstream (egui/winit/zbus) must move. Re-run `cargo audit` each release |
| Android/iOS release signing keys and store accounts | n/a | Held by the owner |

No open critical, high or medium finding remains in the apps and the vault/web paths. The
open critical/high items above all live in the hosted omegagiven.com service and need the
owner (billing, legal, DNS).

## Re-run

`forge run 'cargo install cargo-audit --locked; cargo audit'`,
`node --test hosted/test/*.test.js`, `forge run cargo test -p og-paper account`.
