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
| `pkg/` | the wasm build (not in git; `scripts/build-web.sh` or a runner) |

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
| App Store / releases | `packaging/apple/`, `.github/workflows/` | |

## Build, test, deploy

- Native: `cargo run -p og-paper`; tests `cargo test -p og-paper` (on a runner: `forge run --on og-host1 …`).
- Web: `scripts/build-web.sh`; hosted tests `node --test hosted/test/*.test.js`, browser
  `hosted/test/run-browser-e2e.sh`; vault `tests/vault/run-e2e.sh`, `tests/vault/run-native.sh`.
- Vault test path (web): `scripts/deploy-vault-web.sh`. Android private mirror:
  `scripts/publish-android-vault.sh`. Public web: the `web-dist` branch (HQ).
