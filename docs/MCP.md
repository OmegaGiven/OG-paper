# AI assistants (MCP)

A page server (`og-paper --serve-dir`, or the Docker image) speaks the
[Model Context Protocol](https://modelcontextprotocol.io) at `/mcp`. With it,
Claude or any other MCP-capable assistant can look at your pages and work on
them with you: explain what's on a page, tidy a layout, sketch a diagram,
wireframe or kanban board from a sentence, fill in a table, review a design
and apply the changes you agree to.

Anything the assistant adds or changes shows up live for everyone on the
page, like any other edit, and can be undone in the app.

## Connect

The address is your server's address plus `/mcp` and a server key:

```
http://<server>:8991/mcp?k=<server key>
```

The server key is the `k=` part of the server link printed in the log
(`docker compose logs og-paper`). Use the **view-only** link's key for an
assistant that should only read and look; use the full key to let it draw
and edit. The key can also go in a header instead of the address:
`Authorization: Bearer <server key>`.

### Claude Code

```sh
claude mcp add --transport http og-paper "http://nas:8991/mcp?k=e…"
```

or, keeping the key out of the address:

```sh
claude mcp add --transport http og-paper http://nas:8991/mcp --header "Authorization: Bearer e…"
```

Claude Code connects from your machine, so a server on your LAN or tailnet
works as it is.

### Claude (claude.ai, desktop and mobile apps)

Settings › Connectors › **Add custom connector**, name it "OG Paper" and paste
the `/mcp?k=…` address. Then turn it on in a chat from the tools menu.

Custom connectors are reached from Anthropic's servers, not from your
device, so the server must be reachable from the internet over HTTPS:
put it behind a TLS proxy with a public name (Tailscale Funnel, Caddy,
Cloudflare Tunnel, …) and use `https://your-host/mcp?k=…`. If you'd rather
not expose it, use Claude Code (above), or bridge it into the desktop app
locally with [`mcp-remote`](https://www.npmjs.com/package/mcp-remote) in
`claude_desktop_config.json`:

```json
{"mcpServers": {"og-paper": {"command": "npx",
  "args": ["mcp-remote", "http://nas:8991/mcp?k=e…", "--allow-http"]}}}
```

### Other assistants

Any client that speaks MCP over Streamable HTTP works the same way. The
server answers every request with plain JSON (no sessions, no event
streams) and supports protocol versions 2025-03-26, 2025-06-18 and
2025-11-25.

## What the assistant can do

| Tool | Does | View key |
| --- | --- | --- |
| `list_pages` | the server's pages: id, name, last change | ✓ |
| `read_page` | what's on a page: each thing's id, kind, box and details (text, colours, sizes, shape kind, table cells), the box around everything; filter by region or kind | ✓ |
| `view_page` | a picture of the page, a region or some things (PNG, up to 2048 px) | ✓ |
| `list_fonts` | the fonts texts can use | ✓ |
| `add_to_page` | texts, shapes (rect, ellipse, diamond, triangle, star, polygon, line, arrow), tables and freehand strokes, many at once, in one undo step | |
| `edit_page` | move, resize, rotate, restyle (text, colours, fill, width, font, …), bring to front / send to back and delete, by id | |
| `create_page`, `rename_page`, `delete_page` | manage pages (a deleted page's file is kept as `<id>.ogp.deleted`) | |

Pages can be named by id or by name. Two prompts come with it: **design**
(make something on a page from a brief) and **review** (explain a page
and suggest improvements).

When it connects, the assistant is told how the canvas works (coordinates,
endless zoom, text sizes) and how to work well on it: look first, place
new things in free space on a grid, build a layout in one go, then look
again and fix what's off.

The tools are the [automation API](PLUGINS.md#automations-the-server-api)'s
commands, so anything here can be scripted too.

## Keep it safe

- The full key can change every page. Give an assistant the view key unless
  it needs to draw, and treat any `/mcp?k=` address like a password: it may
  end up in proxy logs and connector settings.
- Text on a page is read by the assistant. Someone who can write on a page
  could write instructions there; review what an assistant proposes before
  letting it make big changes.
