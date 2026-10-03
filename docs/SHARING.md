# Sharing a canvas

OG Paper canvases are local-first: every device keeps its own full copy,
and copies merge. Sharing is a choice of how changes travel between
copies; the merge is the same in every case.

## How merging works

- Every canvas has an id that its copies share; every device has a peer id.
- Strokes are never edited in place: an edit hides the old stroke and adds a
  new one. Each show/hide is logged with a hybrid logical clock stamp
  (wall time, counter, peer), which orders events the same way on every
  device whatever their clocks say.
- Merging two copies takes every stroke either has; each stroke shows if its
  latest event says so. Merge order never changes the result, and merging
  the same copy twice changes nothing.
- When two people edit the same stroke apart, those are rival edits. By
  default the newest edit wins; a host can choose "host wins" or "guest
  wins". The losing version stays in the timeline.
- Undo is per person. Bookmarks, camera position and settings stay per
  person.

The log lives in `.ogp` files (format 0.5: `meta.canvas_id`, tables
`sync_events` and `sync_replaces`) and in web copies (`.ogpt` v5).
Canvases saved before get a log derived from their strokes, identically on
every device, so old copies still merge.

## Ways to share

| Way | Server | Live | Works offline | Where |
| --- | --- | --- | --- | --- |
| Merge copy | none | no | yes | Settings ⚙ > Merge copy |
| Save changes | none | no | yes | Settings ⚙ > Save changes |
| Sync folder | none (Syncthing, Dropbox, Drive, USB) | every few seconds | yes | Settings ⚙ > Sync folder |
| Host this canvas | your desktop app, or `og-paper --serve` | yes | guests keep drawing; sync on reconnect | Share live |
| Host in this browser | none (WebRTC) | yes | needs a new invite after a drop | Share live (web) |
| Relay | `og-paper --relay` | yes | yes: changes wait at the relay | Share live > Share via relay |
| Server (many pages, e.g. on a NAS) | `og-paper --serve-dir` or Docker | yes | yes: each page reconnects when opened | Settings ⚙ > Pages |

### Merge copy and Save changes

Send someone a copy (Export > OG Paper copy makes an `.ogpt` on the web, an `.ogp` on desktop). When they send
theirs back, Merge copy brings their work in; the view flies to what
changed, which glows blue (new) or shows as a red ghost (removed). Save
changes writes only what the other copy lacks since your last merge with it.

### Sync folder

Pick a folder that a sync tool keeps in step across your devices. Each
device writes only its own file (`og-paper-<canvas>/<peer>.ogpt`) and
merges everyone else's as they change, so the sync tool never sees a
conflict. On the web this needs a Chromium browser (File System Access).

### Host this canvas

Share live > Host this canvas (desktop) serves the canvas on port 8991 and
shows an edit link and a view link. Guests open the link in the app (Share
live > Join) or in a browser (`…/app/#join=ws://host:8991&k=KEY`). Each
guest keeps a copy on their device; if they lose the connection they keep
drawing, and their changes apply to the master when they reconnect.

Headless, for a homelab or container:

```
og-paper --serve canvas.ogp [--port 8991] [--public wss://paper.example.com]
docker build -t og-paper . && docker run -d -p 8991:8991 -v og-paper:/data og-paper
```

`--public` sets the address printed in the links (behind a proxy or in a
container). Browsers on an `https://` page can only reach `wss://`
addresses: put the host behind a TLS proxy (for example Tailscale serve).

### Host in this browser (no server)

Share live > Host in this browser makes one-time invite links (can draw, or
view only). The guest opens one and sends back a reply code; once the host
pastes it, both draw together over a WebRTC data channel. Public STUN is
used to cross NATs; networks that need a relay (TURN) will not connect.
Keep the host's tab open.

### Server with many pages (NAS)

```
docker build -t og-paper .
docker run -d --name og-paper -p 8991:8991 -v og-paper:/data og-paper
docker logs og-paper        # shows the server links
```

or without Docker: `og-paper --serve-dir ./pages [--port 8991] [--public wss://host]`.

The log prints two server links: one that can make, rename and delete pages,
and a view-only one. In the app open Settings ⚙ > Pages, paste a server link
under Servers and press Add. The server's pages appear; Open joins one and
keeps a copy on this device, listed under "On this device". Opening that copy
later reconnects by itself, so work done offline goes up. "+ Page" makes a
new page on the server (with the editing link). Each page keeps its own
keys; set `OGP_SERVER_KEY` to choose the server key yourself.

### Relay

```
og-paper --relay [--port 8993] [--data ./og-paper-relay]
```

Share live > Share via relay makes a relay link for the canvas. Everyone
syncs whenever they connect, even if they are never online at the same
time. The relay keeps the sealed changes on disk and passes live frames
on; it cannot read them.

## Security

- A link carries a key. An edit key is a 32-byte secret; a view key holds
  only the frame key and the public half of the signing key.
- Every live frame is sealed end to end with ChaCha20-Poly1305. Changes
  also carry an Ed25519 signature that every receiver checks, so a view
  link cannot make edits anyone accepts, and whoever carries the frames (a
  relay, a proxy, the network) can neither read nor forge them.
- New links (Share live) rotates the keys: links handed out before stop
  working.
- Pictures travel on demand over live connections (each checked against its
  content hash); relays receive them inline.

## Versions

Older and newer apps, servers and copies keep working together: new kinds
of message get new tags that older apps ignore, new fields are only ever
added at the end of a message or copy (older apps read what they know), and
hello messages carry a protocol version so a server can tell what it talks
to. Links made before end-to-end sealing (plain hex keys) no longer work.

## Limits

- Hosting from the desktop app needs the window open; use `--serve` for an
  always-on host.
- WebRTC needs both people online and a copy-paste of the reply code; it
  cannot reconnect by itself.
- Merge copy is not an undo step (undoing your own strokes afterwards works
  as usual).
