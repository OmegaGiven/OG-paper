# Code map

Where things live in og-paper. Start here; current-state docs are linked per area, history is in
`docs/archive/`.

## Crates (Rust)

| Path | What |
| --- | --- |
| `crates/ogpaper-core` | the canvas model: big-number addresses, camera, strokes, brushes (no GPU) |
| `crates/ogpaper-file` | the `.ogp` file (SQLite) read/write; spec in `spec/` |
| `crates/spike` | Phase 0 renderer spike, `og-spike --bench` frame-time benchmark ([PHASE0.md](PHASE0.md)) |
| `crates/og-paper` | the app: native (winit + wgpu + egui) and the wasm build for the web |

Inside `crates/og-paper/src`:

| File | What |
| --- | --- |
| `lib.rs` | `App`: state, actions, the frame loop, commands (`Cmd`), data dir |
| `render.rs`, `*.wgsl` | drawing on the GPU |
| `ui.rs` | every menu, panel and card (egui), incl. Pages and the account section |
| `edit.rs`, `objects.rs`, `shapes.rs`, `textbox.rs`, … | tools and canvas objects |
| `share.rs`, `seal.rs`, `wire.rs` | the change log, sealing (ChaCha20-Poly1305, Ed25519), frames ([SHARING.md](SHARING.md)) |
| `net.rs` | live connections: host/guest, relay links, WebSocket client (bearer for accounts) |
| `relay.rs`, `hub.rs`, `hubconsole.rs`, `hubpage.rs`, `pages.rs` | self-hosted relay / page server and the Pages panel's servers |
| `account.rs` | native accounts: OG Studio (poll sign-in) and vaults (PKCE + loopback), profiles, tokens ([VAULT.md](VAULT.md), [NATIVE-SIGN-IN.md](NATIVE-SIGN-IN.md)) |
| `android.rs` | Android only: the bar/cutout insets (JNI on the Activity) fed to egui as the safe area, so controls sit below the status bar; opt-in full screen (window flag, set once at start); back button = Esc |
| `egui_io.rs` | winit events into egui: `egui-winit` natively, its own translator on the web. Touch: every build hit-tests where a finger lands (web here, Android/iOS in `lib.rs` `window_event`), so a tap on a menu or panel goes to it, not to the canvas (which would close the menu) |
| `web.rs` | the wasm bridge to `web/app/og-web.js` (`og_*` exports, events) |
| `mcp.rs`, `script.rs`, `plugin.rs` | MCP server, commands API, plugins ([MCP.md](MCP.md), [PLUGINS.md](PLUGINS.md)) |
| `export.rs`, `import.rs`, `pdf.rs`, `snapshot.rs` | files in and out |

## Web app (`web/app/`)

| File | What |
| --- | --- |
| `index.html`, `boot.js` | the page; `boot.js` starts the app (no inline script: strict CSPs) |
| `og-web.js` | the shell around the wasm: storage (IndexedDB), panels, sockets, menus |
| `og-hosted.js` | account panel for OG Studio and vaults, live room glue, storage warnings ([HOSTED.md](HOSTED.md)) |
| `og-vault.js` | vault mode: profiles, PKCE sign-in, `VaultProvider` ([VAULT.md](VAULT.md)) |
| `vault/og-profiles.js`, `vault/og-app-login.js` | copies of og-vault `clients/og-profiles` |
| `og-chat.js` | chat on hosted pages |
| `sw.js` | service worker (offline) |
| `vendor/pdfjs/` | our own pdf.js 4.10.38 (no third-party script origin) |
| `pkg/` | the wasm build (not in git; `scripts/build-web.sh` or a runner) |
| `paper-*.png`, `paper*.svg` | the og-design kit's `paper` tile (`tools/vendor.sh tile paper web/app`, kit 0.20.1): manifest, apple-touch (`paper-180.png`) and favicon (`paper-symbol.svg`) are the symbol only; `paper.svg` / `paper-1024.png` (symbol + PAPER) are for sites and stores. Delete the `OG-DESIGN-VERSION` stamp vendor.sh writes (it names the forge host). Android launcher: `vendor.sh launcher paper crates/og-paper/res`; iOS: `packaging/apple/icon-ios-1024.png` (kit `paper-ios-1024.png`). `packaging/make-icons.py` draws only the Mac icon, store art source and hub favicon |

## Hosted (OG Studio server, `hosted/`)

Cloudflare Worker package mounted by omegagiven.com/ogstudio.org: `src/api.js` (HTTP API),
`room.js` (Durable Object per page), `oauth.js` (native app sign-in), `quota.js`, `chat.js`,
`schema.js`; tests in `hosted/test/`. See [HOSTED.md](HOSTED.md). Deploy: HQ bumps omegagiven-com's
`vendor/og-paper` (this repo's `main`) and `public/paper` (branch `web-dist`).

## Flows

| Flow | Doc | Code |
| --- | --- | --- |
| Sign-in (OG Studio or vault), profiles, switching | [VAULT.md](VAULT.md) | `og-vault.js`, `og-hosted.js`, `account.rs` |
| Sync, offline merge, rooms | [SHARING.md](SHARING.md), [HOSTED.md](HOSTED.md) | `share.rs`, `net.rs`, `hosted/src/room.js` (vault: og-vault `paper.rs`) |
| Self-hosted server | README "Run your own server" | `relay.rs`, `hub*.rs`, `Dockerfile` |
| Store releases (App Store, Google Play, GitHub) | [RELEASING.md](RELEASING.md) | `.github/workflows/apple.yml`, `asc.yml`, `play.yml`, `release.yml`; `packaging/apple/asc/`, `packaging/android/`, `scripts/android-aab.sh`, `scripts/play-release.sh` |

Security: [SECURITY-AUDIT.md](SECURITY-AUDIT.md) (current pass, open owner items), [LAUNCH-AUDIT.md](LAUNCH-AUDIT.md) (hosted server).

## Build, test, deploy

- Native: `cargo run -p og-paper`; tests `cargo test -p og-paper` (on a runner: `forge run --on og-host1 …`).
- Web: `scripts/build-web.sh`; hosted tests `node --test hosted/test/*.test.js`, browser
  `hosted/test/run-browser-e2e.sh`; vault `tests/vault/run-e2e.sh`, `tests/vault/run-native.sh`.
- Vault test path (web): `scripts/deploy-vault-web.sh`. Android private mirror:
  `scripts/publish-android-vault.sh`. Update mirrors (web + APK, signed, leak-audited):
  `scripts/release-mirror.sh --apk` (private :8499) and `scripts/release-mirror.sh --public --apk`
  (ogstudio.org/releases; then the demo vault's `follow.sh`). Public web: the `web-dist` branch (HQ).
- Stores: GitHub Actions on the GitHub mirror (after `forge publish og-paper`): Apple release
  (`apple.yml`), Google Play release (`scripts/play-release.sh` via the forge; `play.yml` builds only); see [RELEASING.md](RELEASING.md).
