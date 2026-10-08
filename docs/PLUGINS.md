# Plugins, packs and automations

There are three ways to add to OG Paper, from no code to full programs:

| | What it adds | Code | Where |
| --- | --- | --- | --- |
| **Pack** (`.ogpack`) | toolbars, tools (brushes with all their settings, shapes, text styles), stickers | none | Settings ⚙ > Plugins > Install |
| **Plugin** (`.wasm`) | buttons that draw or arrange things | any language that builds WebAssembly | Settings ⚙ > Plugins > Install |
| **Automation** | anything a script or service does to pages on a server | any language with WebSockets | the server's `/api` |

Plugins and automations hand the app the same JSON **commands**.

## Commands

Coordinates are points at the home view (Settings ⚙ > Bookmarks > Home),
measured from its centre: x to the right, y down. Sizes are in the same
points, colors `#rrggbb` or `#rrggbbaa`.

```json
[{"add": "text", "x": 0, "y": 0, "text": "Hello", "size": 24, "color": "#c82850"},
 {"add": "stroke", "points": [[0, 40], [80, 60], [160, 40]], "width": 3, "color": "#1e5ac8"},
 {"add": "shape", "kind": "rect", "x": -100, "y": 100, "w": 200, "h": 80,
  "fill": "#ffd166", "color": "#333333", "width": 2, "neat": true},
 {"get": "texts"},
 {"say": "Done"}]
```

- `add: text` — `text`, `x`, `y` (top left), `size`, `color`.
- `add: stroke` — `points` (`[[x, y], …]`, up to 100 000), `width`, `color`.
- `add: shape` — `kind` (rect, ellipse, diamond, triangle, star, polygon,
  line, arrow), `x`, `y`, `w`, `h` (a line or arrow runs from x,y to
  x+w,y+h), `color` (outline), `fill`, `width`, `neat` (false for a
  hand-drawn look).
- `add: audio` — an audio clip (a small player anyone on the page can
  play): `x`, `y` (top left), `w`, `h`, and either `data` (a sound file,
  base64: WebM, Ogg, MP4/M4A, WAV or MP3, at most 2 MB) with an optional
  `duration_ms`, or `"record": true` with `max_seconds` (1–120, default
  30): the app records from the microphone, with a bar to stop or cancel,
  and puts the clip in the middle of the screen when the person stops.
  Recording needs the `microphone` permission (below) and works in the
  web app for now; the desktop and mobile apps keep and show clips.
- `get: texts` — every text on the page: `{"text", "x", "y", "size"}`.
- `say` — shows a short message.

One run is one undo step, and syncs to everyone on the page like any
other edit. Each command gets a result in order: `{"ok": true}` (plus
`"texts"` for `get`) or `{"ok": false, "error": "…"}`. A view-only copy
refuses `add`.

## Packs

A pack is a text file. Settings ⚙ > Plugins > **Share my toolbars** saves
yours as one; edit its `name` line and hand it out.

```text
og-paper-pack 1
name Watercolor set
b 0 Washes
h 0 0 pen …
i 3 pen …
sticker Leaf <base64 of a library sticker (.ogps)>
```

`b`/`h`/`y`/`i` lines are the toolbar file's own (toolbars, their slots,
their hold tools, and inventory tools). Installing adds the pack's toolbars as
"<pack>: <toolbar>" (switch to them with `[` / `]` or the toolbar list),
puts its tools in free inventory slots and its stickers in the library.
Nothing of yours is replaced.

## Plugins (WebAssembly)

A plugin is a `.wasm` module. The app runs it in its own interpreter
(wasmi), the same on desktop, Android and the web, and gives it nothing but
its input: no files, no network, no clock. Every button press starts a fresh
instance with a step limit (a stuck plugin stops with a message instead of
freezing the app) and a 64 MB memory cap. Its answer is commands, so it can
only do what commands do, as one undo step.

A plugin exports `memory` and three functions:

| Export | Signature | Does |
| --- | --- | --- |
| `og_alloc` | `(len: i32) -> i32` | room for `len` bytes of input; returns where |
| `og_manifest` | `() -> i64` | `(ptr << 32) \| len` of UTF-8 JSON: the manifest |
| `og_run` | `(button: i32, ptr: i32, len: i32) -> i64` | given the input JSON, `(ptr << 32) \| len` of the commands |

The manifest:

```json
{"name": "Starter kit", "version": "1.0",
 "description": "Example plugin: a dot grid, a spiral and a text count",
 "buttons": [{"id": 1, "label": "Dot grid here"}, {"id": 2, "label": "Spiral"}]}
```

### Permissions

A plugin that needs more than drawing lists it in its manifest:
`"permissions": ["microphone"]` (the only one so far). The plugin itself
still gets no access to anything: the app does the work (here, recording
with its own bar to stop or cancel), and only once the person has allowed
it in Plugins (Allow / Stop allowing, kept per plugin). Until then the
command fails with a message saying what to allow. Automations (the
server API) never get permissions.

The app comes with two plugins, ready to install from Plugins: **Audio
notes** (`plugins/examples/audio-notes`: record a voice note, 30 s or
2 min) and the **Starter kit**.

The input to `og_run`:

```json
{"button": 2,
 "view": {"x": -683, "y": -384, "w": 1366, "h": 768},
 "texts": [{"text": "Hello", "x": 0, "y": 0, "size": 24}]}
```

`view` is the part of the canvas on screen, in command coordinates, so a
plugin can draw where the person is looking. The answer is a JSON array of
commands, or `{"commands": [...]}`.

### Example

[`plugins/examples/starter`](../plugins/examples/starter) is a complete
plugin in plain Rust with no dependencies (dot grid, spiral, text count);
the built file is [`plugins/examples/starter.wasm`](../plugins/examples/starter.wasm).

```
cd plugins/examples/starter
cargo build --release --target wasm32-unknown-unknown
# install target/wasm32-unknown-unknown/release/og_paper_starter_plugin.wasm
```

Any language that makes a WebAssembly module with these exports works
(C, Zig, AssemblyScript, TinyGo, …).

### Where plugins are kept

Desktop: `~/OG Paper/plugins/` (one `.wasm` each; drop a file there and it
loads next start). Web: in the browser's storage. Remove deletes it.
Android runs plugins kept in its app folder, but has no install picker yet.

## In the browser

The web app takes the same commands from the page itself, for devtools
snippets, bookmarklets and extensions:

```js
ogPaper.run([{ add: 'text', x: 0, y: 0, text: 'Hello' }]);
```

## Automations (the server API)

A page server (`og-paper --serve-dir`, or the Docker image) takes JSON over
a WebSocket at `/api`, e.g. `ws://nas:8991/api`. Each message is one JSON
object; an `id` you send comes back on its reply.

| Send | Reply |
| --- | --- |
| `{"auth": "<server key>"}` (first) | `{"ok": true, "can_edit": true}` — the view key gives `false` |
| `{"cmd": "pages"}` | `{"pages": [{"page", "name", "changed"}]}` |
| `{"cmd": "new_page", "name"}` | `{"page": "<hex>"}` |
| `{"cmd": "rename_page", "page", "name"}` / `{"cmd": "delete_page", "page"}` | `{"ok": true}` |
| `{"cmd": "run", "page", "commands": [...]}` | `{"results": [...]}` |
| `{"cmd": "texts", "page"}` | `{"texts": [...]}` |
| `{"cmd": "watch", "page"}` / `unwatch` | then `{"event": "changed", "page"}` whenever it changes |

The server key is the `k=` part of the server link the log prints (or
`OGP_SERVER_KEY`). A wrong key closes the connection. Edits made through
the API reach everyone on the page at once, and apps that open it later.

Helpers: [`clients/js/ogpaper.mjs`](../clients/js/ogpaper.mjs) (browser or
Node 22+, no dependencies) and
[`clients/python/ogpaper.py`](../clients/python/ogpaper.py) (`pip install
websockets`).

```js
import { connect } from './ogpaper.mjs';
const og = await connect('ws://nas:8991', 'e…');
const [page] = await og.pages();
await og.run(page.page, [{ add: 'text', x: 0, y: 0, text: 'Build passed ✔' }]);
og.watch(page.page, () => console.log('someone drew'));
```

```python
og = await ogpaper.connect("ws://nas:8991", "e...")
page = (await og.pages())[0]["page"]
print(await og.texts(page))
```

Use `wss://` behind a TLS proxy when the server is reachable from outside.
