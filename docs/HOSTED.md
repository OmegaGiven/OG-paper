# Hosted OG Paper (accounts, sharing, live rooms)

A website that already has accounts can host OG Paper for its users: each
account keeps its pages on the site, sees them as soon as it signs in,
draws on them together with others live, and shares, copies or gives them
away. Signed-out visitors keep today's behaviour (pages in the browser).

It is built for a Cloudflare Worker site (omegagiven.com is the first):

- **One Durable Object per page** (`PaperRoom`): the live room and the
  page's change log, in the object's own SQLite storage.
- **D1** for the page directory: pages, members, links, quotas, reports.
- **R2** (optional) for frames over 1 MB; without it they are split across
  rows in the room.
- **No user system of its own.** The site says who is signed in through an
  auth adapter.

Everything lives in [`hosted/`](../hosted) as a package (`og-paper-hosted`):
`src/` (API, room, quota, protocol), `migrations/` (D1 schema),
`adapters/omegagiven.js`, `client/oplog.js` (reference client),
`example/` (a whole hosted OG Paper in one Worker, runnable with
`wrangler dev`), and `test/`.

## How it fits OG Paper's sync model

Nothing new is invented for merging. OG Paper already merges copies with
an add-only log: strokes are never edited in place, every show/hide is
stamped with a hybrid logical clock, and rival edits settle by the newest
edit, the same on every device in any order ([SHARING.md](SHARING.md)).
Its relay (`og-paper --relay`) already stores sealed changes for people
who are not online together and passes live frames on.

A hosted room **is that relay**, plus who-may-do-what, quotas, resume and
tiles. So:

- Every OG Paper app can use it unchanged: it joins
  `wss://<site>/paper/api/pages/<id>/relay?k=<key>&c=<canvas>` as a
  relay link.
- Frames stay sealed end to end (ChaCha20-Poly1305, changes signed with
  Ed25519). The room never reads a page. A view key cannot make changes
  any app accepts, even if a room were to pass them on.
- Offline edits merge on reconnect: the app keeps its copy (IndexedDB on
  the web), and when the room has sent what it keeps (`READY`), the app
  sends what the room lacks.
- Presence: apps send their view (`Presence`, ~7/s while moving) and
  strokes being drawn (`Wet`, 20/s) as passed-on frames, so others see
  cursors, views and names live. The room adds a list of who is on the
  page, with names from their accounts (a text frame, see below), which
  the web page shows in My account.

## Live protocol

One WebSocket per page. Binary frames are the relay protocol (tag, then
body):

| From | Frame | Meaning |
| --- | --- | --- |
| app | `ROOM` + 32 bytes | join (the room id derived from the key; the first key seen pins the room, others are refused) |
| app | `PUT ` + sealed | a change: kept, charged to the owner, passed on |
| app | `EPH ` + sealed | a view or a stroke in progress: passed on, never kept |
| app | `CKPT` + sealed | a full copy: kept in place of every frame the sender had when it made it |
| app | `PUTT` + tiles + sealed | a change that touches these tiles |
| app | `TCKP` + 1 tile + sealed | a full copy of one tile |
| room | sealed | a kept or passed-on frame |
| room | `SEQ ` + u64 + sealed | a kept frame with its place in the log (apps that asked) |
| room | `READY` | everything kept has been sent |

Tiles are a u8 count, then a u8 length + UTF-8 each.

Text frames are the room's own control channel (JSON). The web page reads
them; the app never sees them, so apps that predate hosting work as they
are.

| From | Message |
| --- | --- |
| app | `{t:'hello', since, seq:true, tiles, room}`: join instead of `ROOM`; only frames after seq `since`; only these tiles (null: all) |
| app | `{t:'subs', add, remove}`: tiles came into or left view; the room sends what new tiles hold |
| room | `{t:'ready', head}` / `{t:'welcome', role, head, bytes}` |
| room | `{t:'ack', seq}`: your change was kept as seq |
| room | `{t:'peers', list:[{user, name, role}]}`: who is here (checked identities) |
| room | `{t:'full', used, quota, msg}`: not kept: the owner's storage is full |
| room | `{t:'denied', msg}`: not allowed (a viewer writing) |
| room | `{t:'kicked', msg}`: access changed; the socket closes (4003) |
| room | `{t:'compact'}`: please send a checkpoint |
| both | chat: `chat`, `chat-del`, `chat-react`, `typing` and the room's answers, see [Chat](#chat) |

### Infinite zoom and big pages: tiles and resume

A page's log could hold far more than one screen needs, at depths of
10^45. Two things keep traffic to what matters:

- **Resume.** Each kept frame has a seq. A client that says `since: N`
  gets only frames after N, so a reconnect after a short drop costs only
  what changed (tested: one frame after five). Checkpoints keep this
  correct: a checkpoint covers only frames up to what its sender had
  seen, and frames after it stay.
- **Tiles.** A change can name the tiles it touches; a client can
  subscribe to the tiles in view and add more as it pans or zooms. The
  room sends only matching frames (and untagged ones once), and passes on
  live changes only to sockets whose tiles they touch. A tile is a
  string the app picks. The scheme meant for infinite zoom: tile =
  `<band>/<x>/<y>`, where band = the stroke cell's level rounded down to a
  multiple of 8 and x, y = its ancestor cell at that band's level. A view
  at zoom band B subscribes to the tiles over the view in bands up to B+1
  (bands above are one or two tiles each). Strokes in deeper bands are too
  small to see from there and arrive when you zoom into them.

  Rooms refuse whole-page checkpoints from clients that only see some
  tiles; per-tile checkpoints (`TCKP`) compact a tile instead.

The room side of both is done and tested (`hosted/test/collab.test.js`);
the reference client (`hosted/client/oplog.js`) uses both. **The Rust app
still joins as a plain relay client**: it gets the latest checkpoint plus
the changes after it, and sends a checkpoint once that backlog passes
1 MB, so a page loads whole. With a 25 MB quota a page is at most a few MB
of strokes, which loads fine; teaching the app to send `PUTT` and follow
the view with `subs` is the next step for very large pages.

## Data model

**D1** (`hosted/migrations/0001_og_paper.sql`, tables prefixed `paper_` so
they can share the site's database):

- `paper_accounts(user_id, quota, used, blocked)`: one row per account
  that has used OG Paper. `used` = Σ bytes its pages keep + quota busy rooms
  took ahead.
- `paper_pages(id, owner_id, name, canvas, edit_key, view_key, bytes,
  blocked, ...)`: `owner_id` is the billing owner. The keys are the
  page's sealing keys, made in the browser when the page is made, handed
  out only by role.
- `paper_members(page_id, user_id, role, via_link)`: owner (billing owner
  and co-owners), editor, viewer.
- `paper_links(id, token_hash, page_id, role)`: "anyone with the link";
  only a hash of the token is kept.
- `paper_reports(page_id, reporter_id, reason, resolved_at, resolution)`.

**Room** (one per page, SQLite): `frames(seq, kind, tiles, author, at,
size, body, store)`, `chunks` (frames over 1 MB without R2), `subs` (tile
subscriptions per socket), `meta` (page, owner, pinned room, lease).

**R2** (optional, `PAPER_BUCKET`): `og-paper/<page>/<seq>` for frames over
1 MB.

## Permissions

| | viewer | editor | owner (co-owner) | billing owner | admin |
| --- | --- | --- | --- | --- | --- |
| open, see live changes, presence | ✓ | ✓ | ✓ | ✓ | view only |
| draw (changes kept) | | ✓ | ✓ | ✓ | |
| rename | | ✓ | ✓ | ✓ | |
| share with people, change roles, make/turn off links | | | ✓ | ✓ | |
| make a copy into own account | ✓ | ✓ | ✓ | ✓ | |
| give away (transfer billing), delete | | | | ✓ | delete |
| leave | ✓ | ✓ | ✓ | | |
| report | ✓ | ✓ | ✓ | | |
| take down, delete, set quotas, read reports | | | | | ✓ |

- Viewers get the **view key** only, so even their app cannot sign
  changes; the room also refuses their `PUT`/`CKPT` and never stores or
  passes them on.
- **Revoking** someone (or demoting them, or turning off the link that
  brought them in) closes their live connection at once (the API tells
  the room) and every later request answers 404.
- A page someone cannot see answers exactly like a page that does not
  exist. There is **no public listing**: page ids are 128-bit random, link
  tokens 192-bit random (stored hashed), and the only lists are "my pages"
  and "shared with me".
- Sharing is by username or by an email an account signed in with.
- **Link sharing** (view or edit links) adds whoever opens it, signed in,
  as a member (it then shows under Shared with me), until the link is
  turned off. Links without signing in are off by default
  (`linkAnonymousView`); when on, they only ever view.
- **Co-owners** can share and manage links; the **billing owner** pays the
  quota and alone can delete the page or give it away. Giving away moves
  the page's bytes to the new owner's quota (refused if they have no
  room); the old owner can stay as a co-owner or leave.
- **Make a copy** makes a new page in my account with new keys and fills
  it from the open page (in the app: the room cannot read pages to copy
  them). It counts toward my quota.
- The session is a cookie, so everything that changes something or opens
  a room must come from the site itself (`Origin` / `Sec-Fetch-Site`
  checked).

## Quotas

- **Free accounts: 25 MB** (proposed; `PAPER_FREE_QUOTA` in bytes, and per
  account by an admin). Everything a page keeps counts: strokes, text,
  history, pictures and PDFs (they travel inside the changes). For scale:
  a stroke is roughly 0.1-1 KB, so 25 MB is tens of thousands of strokes,
  or about 5-10 phone photos.
- Plus at most **100 pages** per account, **25 people** per page, **5 links**
  per page.
- The **room enforces it**, not the app: before keeping a change it takes
  quota from the billing owner's account with one conditional UPDATE
  (`used + n <= quota`), so an account never goes over however many of
  its pages are busy. Busy rooms take 256 KB at a time ahead and hand back
  what they did not spend when they go quiet (30 s after the last person
  leaves), so `used` is exact at rest and D1 sees one write per 256 KB, not
  per stroke.
- **When full**, the change is not kept or passed on, and the person drawing
  gets "Your storage is full: this change was not saved to your account.
  Export or delete pages to free space." (or, on someone else's page, "The
  owner's storage is full… make a copy into your own account"). The web
  app shows a card with Export and Delete pages. Their work stays in their
  browser and goes up once there is room (reopen the page).
- **Freeing space:** deleting a page gives back everything it kept;
  checkpoints (which replace many changes with one copy) give back the
  difference; giving a page away moves its bytes.
- **Caps per frame:** a change (including pictures sent inline) ≤ 1 MB, a
  passed-on frame ≤ 16 KB, a checkpoint ≤ 25 MB; anything bigger is
  refused with a message. Pictures larger than 1 MB therefore cannot be
  sent yet (see Decisions).
- Usage shows in My account: "3.2 MB of 25 MB used · 4 of 100 pages", and
  live while drawing: the room tells the owner's open pages how full the
  account is each time it takes quota (a `{t:'usage'}` note).
- **Warnings, and where to sell storage:**
  - At 80% and again at 95%, a "Storage nearly full" card: when it is full,
    new edits stay on this device only and are not saved to the account.
  - When full, a "Storage full" card each time a change is refused.
  - Both cards, and the usage line in My account, offer **Get more
    storage**, linking to the site's billing page: `PAPER_UPGRADE_URL`
    (or `upgradeUrl` in the config, or `<meta name="og-paper-upgrade">`).
    Without one set, no offer shows. On someone else's page the full card
    suggests making a copy instead.
- **Closing the tab with work not in the account** makes the browser ask
  first (the standard "Leave site?" prompt; browsers don't allow custom
  text). That covers edits the room refused (full), edits made offline or
  still on their way (the room confirms every kept change with
  `{t:'kept'}`), and, on the hosted site, a canvas that is only in this
  browser (signed out, or not kept in the account). A chip at the top of
  the screen says which: "⚠ Not saved to your account" or "Only in this
  browser: sign in to save to the cloud". Tap it for My account.

## Abuse basics

- **Rate limits:** per socket in the room (60 frames/s, 240 burst; 512 KB/s,
  4 MB burst; enough for drawing with presence), and per account or IP on
  the API through the adapter (`limit(env, kind, key)`: `api`, `write`,
  `create`, `ws`), mapped onto omegagiven.com's `RL_*` bindings.
- **Size caps** per frame (above), per page name, per report.
- **At most 32 people** in one room at once.
- **No public listing** (above).
- **Reports:** anyone who can see a page can report it (`POST
  /pages/:id/report`); the site's `onReport` hook is called (send it to
  Signal, email…). Admins (`PAPER_ADMINS`) list reports, look at a page
  (view only), take it down (everyone is disconnected; it answers 451),
  delete it, block an account, set its quota, or recount its usage.

## HTTP API

Mounted at `/paper/api`. JSON in and out; errors are `{error}` with a
status.

| | |
| --- | --- |
| `GET /me` | `{user, quota, used, pages, maxPages, linkSharing, admin}` (user null when signed out) |
| `GET /pages` | `{mine, shared, usage}` |
| `POST /pages {name, canvas, edit, view}` | a new page (keys made in the browser) and how to open it |
| `GET /pages/:id` | the page; members and links for owners |
| `PATCH /pages/:id {name}` / `DELETE /pages/:id` | rename (editor+) / delete (billing owner) |
| `POST /pages/:id/open` | `{link, role, canvas, name}` (the relay link with the key for my role) |
| `GET /pages/:id/relay` | the live room (WebSocket) |
| `POST /pages/:id/copy {canvas, edit, view}` | a copy in my account: `{page, open, from}` |
| `POST /pages/:id/members {who, role}` | share (owners) |
| `PATCH/DELETE /pages/:id/members/:uid` | change role / remove (owners); remove myself = leave |
| `POST /pages/:id/links {role}` / `DELETE /pages/:id/links/:lid` | make / turn off a link (owners) |
| `GET/POST /links/:token` | what a link opens / open it (adds me) |
| `POST /pages/:id/transfer {who, leave}` | give away (billing owner) |
| `POST /pages/:id/report {reason}` | report |
| `GET /pages/:id/chat` | the chat's settings and kept messages (data export; members, or owners only while chat is off) |
| `PATCH /pages/:id/chat {on, keep}` / `DELETE /pages/:id/chat` | chat on/off, kept or ephemeral / clear it (owners) |
| `/admin/reports`, `/admin/reports/:id`, `/admin/pages/:id` (GET, DELETE), `/admin/pages/:id/block`, `/admin/accounts/:uid`, `/admin/accounts/:uid/recount` | admin |

## The web app

`web/app/og-hosted.js` gives the web app a storage-provider interface
(`LocalProvider`: pages in this browser; `HostedProvider`: pages in the
account; both answer `list`, `create`, `open`, `rename`, `remove`,
`usage`). A site turns hosting on by adding to the app's page:

```html
<meta name="og-paper-api" content="/paper/api">
<meta name="og-paper-signin" content="/account/?next={next}">
```

Then, signed in: the **My account** panel opens by itself with My pages
and Shared with me (or the page last open reopens), **+ New page**, **Keep
this one** (the open browser canvas into the account), per page Share
(people, roles, links), Make a copy, Rename, Give away, Delete, Leave,
Report, storage use, and who is on the page now. Signed out, the panel
offers Sign in and everything else is as before. Pages opened from the
account keep a copy in the browser and reopen offline; changes go up on
reconnect. The app's own Pages panel lists those copies under "On this
device".

## Chat

People on the same page can talk while they draw. It rides the page's
live socket (the room's text frames), so it has the room's permissions:
only people who can open the page can read or write, and a person removed
from the page is disconnected and loses the chat with it.

**In the web app** (`web/app/og-chat.js`): a round chat button (bottom
right, above the pen) appears once two or more people are on the page (or
there is kept history, or unread messages), with an unread badge (red when
someone @mentioned you). It opens a side panel (a bottom sheet on phones)
with who is here (tap someone: go to where they are on the page, mention
them, or for owners remove them from the page), who is typing, the
messages with name and time, quick reactions (👍 ❤️ 😂 🎉 👀 ✅), deleting
your own messages, `@` completion of the people on the page, and a "go to
them" link on messages from people still here (`og_go_to_peer(name)` in
the app flies to their view). Owners also get, in the panel's ⋯ menu and in
the page's Share card: Chat on this page, Keep chat history, Clear chat.
Everyone gets Download chat (a text file).

**Settings (owners and co-owners only):**

- **On by default.** Pages are only ever shared with chosen people (no
  public pages, links need an account), so chat adds no new audience; it
  only shows when someone else is actually there; and it is what people
  drawing together reach for. An owner who doesn't want it turns it off
  per page, and a site turns it off everywhere with `PAPER_CHAT=off`.
- **Ephemeral by default** (messages go once everyone has left the page).
  The point is talking while drawing together; nothing is kept that the
  owner didn't ask for (less to export, back up or moderate, and no D1 or
  quota cost). "Keep chat history" keeps the last 500 messages with the
  page (`PAPER_CHAT_KEEP=on` makes that the site's default).
- **Off:** no chat UI for anyone, and the room refuses chat messages
  (`chat-error`). Kept messages stay and come back if it is turned on again.

**Protocol** (text frames, JSON; who sent it is the socket's checked
account, never a field of the message):

| From | Message |
| --- | --- |
| room | `welcome.chat`: `{on, keep, max, history, reactions}`, or `null` when the site has no chat (an older room sends nothing; the page then shows no chat) |
| room | `{t:'chat-history', list}`: on joining, when chat is on |
| app | `{t:'chat', text, ref}` → everyone: `{t:'chat', msg:{id, user, name, text, at, reacts}}` (the sender's copy carries its `ref`) |
| app | `{t:'chat-del', id}`: own messages; owners may delete anyone's → `{t:'chat-del', id}` |
| app | `{t:'chat-react', id, emoji}`: toggles; only the six listed → `{t:'chat-react', id, reacts:{emoji:[{user,name}]}}` |
| app | `{t:'typing', on}` → the others: `{t:'typing', user, name, on}` |
| room | `{t:'chat-settings', on, keep, …}` / `{t:'chat-clear'}`: an owner changed or cleared it |
| room | `{t:'chat-error', msg, ref}`: refused (off, too long, too fast, not yours, no account) |

**Safety** (applies docs/LAUNCH-AUDIT.md):

- Messages are plain text. The room strips C0/C1 control characters (but
  tab and newline), zero-width spaces, BOMs and the bidi overrides/isolates
  (U+202A-202E, U+2066-2069) that can make text read differently than it
  is, collapses blank lines, and caps a message at 1000 characters
  (counted as characters, so emoji count as one). `<`, `>` and `&` are kept
  as text: the app builds every message with text nodes, never `innerHTML`,
  and makes links only of `http(s)` addresses (`<a target=_blank
  rel="noopener noreferrer nofollow ugc">`). A downloaded chat is a `.txt`.
- Rate limits per account (across all its sockets in the room): messages
  0.5/s with a burst of 8; reactions and deletes 2/s, burst 16; typing
  notes 1/s. Chat frames also count against the socket's frame limit.
- Visitors on an anonymous view link (off by default) can read along but
  not chat. Viewers can chat.
- Reactions are a fixed list; anything else is ignored.
- History: at most `chatHistory` (500) messages per page, in the room's
  own storage (a `chat` table: no D1 change, no Durable Object migration),
  deleted with the page (the room's storage goes) and exported with
  `GET /pages/:id/chat`. Chat does not count toward the storage quota
  (500 × 1000 characters is under 2 MB at most, typically tens of KB).

## Integration steps for omegagiven.com

1. **Get the package.** Copy `hosted/` (src, adapters, migrations) into the
   site, e.g. `vendor/og-paper-hosted/`, or add it as a git dependency.
   Copy the built web app (`./scripts/build-web.sh` here, then `web/app/`
   with `pkg/`) to `public/paper/`.
2. **D1 migration.** Copy `hosted/migrations/0001_og_paper.sql` into
   `migrations/` as the next number (`0011_og_paper.sql`) and apply it
   (`wrangler d1 migrations apply omegagiven-accounts`, `--local` first).
3. **wrangler.jsonc:**
   ```jsonc
   // durable_objects.bindings
   { "name": "PAPER_ROOMS", "class_name": "PaperRoom" },
   // migrations
   { "tag": "v9", "new_sqlite_classes": ["PaperRoom"] },
   // optional: frames over 1 MB
   "r2_buckets": [{ "binding": "PAPER_BUCKET", "bucket_name": "og-paper" }],
   // assets.run_worker_first: add "/paper/api/*"
   // vars: "PAPER_FREE_QUOTA": "26214400", "PAPER_LINK_SHARING": "on",
   //       "PAPER_UPGRADE_URL": "/account/storage"   (where "Get more storage" goes)
   ```
   and the admin list as a secret: `wrangler secret put PAPER_ADMINS`
   (comma-separated user ids).
4. **src/worker.js:**
   ```js
   import { createPaperApi, PaperRoom } from '../vendor/og-paper-hosted/src/index.js';
   import { omegagivenAuth } from '../vendor/og-paper-hosted/adapters/omegagiven.js';
   import { currentUser } from './auth.js';
   import { limited } from './guard.js';
   export { PaperRoom };
   const paper = createPaperApi({
     auth: omegagivenAuth({ currentUser, limited }),
     onReport: (r, env) => {/* e.g. notify the admin */},
   });
   // in fetch(), before the other routes:
   const p = await paper(req, env, ctx); if (p) return p;
   ```
5. **The app's page.** In `public/paper/index.html` add the two meta tags
   (above), with `og-paper-signin` pointing at the site's sign-in
   (`/account/?next={next}` or `/auth/login/google?next={next}`). The
   service worker never caches `/api/`.
6. **Try it** with `wrangler dev` and two browsers (two accounts), then
   deploy. `hosted/example/` is the same wiring in one file, and
   `hosted/test/workerd-smoke.mjs` / `browser-e2e.mjs` run against any
   deployment of it.
7. Update the OmegaEnv tile and the site's links ("OG Paper" → `/paper/`).

## Cloudflare limits and costs

Numbers from Cloudflare's pricing pages (October 2026).

**Workers Free plan** (daily, reset 00:00 UTC; when a limit is hit, further
operations of that kind fail until the reset):

| | Free | What uses it here |
| --- | --- | --- |
| Worker requests | 100,000/day (shared with the whole site) | every API call and every room connection |
| Durable Object requests | 100,000/day | a connection = 1; incoming WebSocket messages count 20:1; outgoing are free |
| DO duration | 13,000 GB-s/day | only while a room is awake: rooms use the Hibernation API, so idle open sockets cost nothing |
| DO SQLite rows written | 100,000/day | ~1 per kept change (one frame row), plus checkpoints |
| DO SQLite rows read | 5 million/day | joining a page reads its frames |
| DO storage | 5 GB per account | all pages' content |
| D1 rows read / written | 5 million / 100,000 per day | lists and sharing; quota: 1 write per 256 KB drawn; page sync once a minute while busy |
| D1 storage | 5 GB | directory only (tiny) |
| R2 | 10 GB-month, 1M Class A, 10M Class B per month, free egress | frames over 1 MB only |
| WebSocket message | 32 MiB received | our caps are far below |
| DO row / BLOB | 2 MB | why frames over 1 MB are split or go to R2 |

**What a drawing person costs:** while drawing and moving, an app sends
about 27 frames a second (20 stroke-in-progress, ~7 view updates), i.e.
about 1.35 DO requests a second. So the free plan's 100,000 DO requests a
day is roughly **20 hours of active pen time a day across the whole site**
(idle people cost nothing), and 100,000 rows written is roughly **90,000
strokes a day**. Storage: 5 GB is 200 full free accounts or many more
light ones.

**At scale** (Workers Paid, $5/month): DO requests 1M/month included then
$0.15/M; duration 400,000 GB-s included then $12.50/M GB-s; SQLite rows
written 50M/month included then $1.00/M; stored 5 GB-month included then
$0.20/GB-month; D1 25 billion rows read and 50M written per month
included, storage 5 GB then $0.75/GB-month; R2 $0.015/GB-month, Class A
$4.50/M, Class B $0.36/M. Rough guide: 1,000 people drawing an hour a
month ≈ 5M DO requests ≈ $0.60; 1,000 accounts at 25 MB = 25 GB ≈ $4/month
of DO storage. The first things to tune at scale are the stroke-in-progress
rate (send it at 10/s instead of 20) and moving page content older than a
checkpoint to R2.

## Tests

- `cd hosted && node --test test/*.test.js` (Node 22.5+, no dependencies;
  in CI): 30 tests with stand-ins for D1, Durable Objects and WebSockets.
  - `collab.test.js`: two and three people editing at once; rival edits of
    one stroke settle the same everywhere; delete against edit; offline
    edits merge on reconnect, fetching only what came after; tiles;
    checkpoints; the Rust app's relay frames unchanged; presence never
    kept.
  - `permissions.test.js`: no access looks like no page; sharing by
    username and email; a viewer cannot write (nor checkpoint, rename,
    share, delete); an editor can draw and rename only; a revoked person
    is disconnected at once and cannot come back; demotion; links and
    revoking them; links off site-wide; co-owners and transfer; leaving;
    origin checks; reports, takedown and admin delete.
  - `quota.test.js`: full accounts are refused and never go over; the owner
    pays for editors; deleting frees space; quiet rooms hand back leases;
    checkpoints free space; copies count toward the copier; transfers move
    bytes or are refused; frame size caps and floods; admin quotas.
- `test/workerd-smoke.mjs`: the same flows against `wrangler dev` (real
  workerd, Durable Objects, D1, WebSockets).
- `test/chat.test.js` / `chat-render.test.js`: chat (identity from the
  session, no access no chat, owners' settings, ephemeral vs kept and the
  500 cap, deleted with the page, moderation, reactions, typing, limits,
  text cleaning, site switch, anonymous visitors) and how the app shows
  messages (HTML stays text, only http(s) links, mentions).
- `test/chat-e2e.mjs`: chat in two browsers (desktop + phone) against
  `wrangler dev`. `test/run-browser-e2e.sh` builds the web app, starts
  `wrangler dev` and runs both browser tests (`forge run
  hosted/test/run-browser-e2e.sh`; `SHOTS=dir` keeps screenshots).
- `test/browser-e2e.mjs`: the real web app (Rust in Chromium, Playwright)
  against `wrangler dev`: account panel on sign-in, new page, sharing,
  Shared with me, live edits both ways, Make a copy, reopening after a
  reload.

## Decisions for the site owner

- **Chat:** on by default per page, ephemeral (see [Chat](#chat)).
  `PAPER_CHAT=off` removes it site-wide; `PAPER_CHAT_KEEP=on` keeps history
  by default.
- **Free quota:** 25 MB proposed (`PAPER_FREE_QUOTA`). 10 MB is enough for
  handwriting-only use; 50-100 MB if photos matter.
- **Link sharing:** on by default, links need a signed-in account. Options:
  off entirely (`PAPER_LINK_SHARING=off`), or allow view-only links
  without signing in (`PAPER_LINK_ANON=on`; costs requests from anyone).
- **Sign-up limits:** OG Paper adds no sign-up of its own; anyone who can
  make an omegagiven.com account gets 25 MB. Consider a cap on accounts per
  day, OG Paper only for accounts with a username, or a smaller quota until
  an account is a few days old.
- **Pictures over 1 MB:** refused today. Either downscale pictures in the
  app before sending (recommended) or raise `maxOpBytes` (up to ~30 MB).
- **Paid storage:** the warnings already link to `PAPER_UPGRADE_URL`. The
  site needs that page (plans, checkout, e.g. the existing Stripe
  supporters flow) and, on payment, to raise the account's quota
  (`POST /paper/api/admin/accounts/:uid {quota}`, or an UPDATE of
  `paper_accounts.quota` from the payment webhook).
- **Admins:** who goes in `PAPER_ADMINS`, and where `onReport` should
  notify.
- **R2:** bind a bucket (free tier is plenty) or leave big frames split in
  the room.
