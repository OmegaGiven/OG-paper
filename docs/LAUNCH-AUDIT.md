# Hosted OG Paper: pre-launch audit

Task #178, 2026-11-01. Scope: OG Paper as hosted at
<https://omegagiven.com/paper/> (og-paper `hosted/` + `web/app/`, wired into
the omegagiven-com Worker), before marketing and paid hosting. Audit only:
no production code was changed.

## How this was checked

| What | Result |
| --- | --- |
| Read: `hosted/src/*` (API, room, quota, protocol), `hosted/adapters/omegagiven.js`, `web/app/og-hosted.js`, `og-web.js`, `sw.js`; omegagiven-com `src/worker.js`, `src/auth.js`, `src/guard.js`, `src/donations.js`, `public/_headers`, `wrangler.jsonc`, `/terms`, `/privacy` (at omegagiven-com `3d59622`) | findings below |
| `node --test hosted/test/*.test.js` (og-host2) | 31/31 pass |
| `cargo test --workspace` (og-host2) | 155/155 pass, 10 suites |
| New: `hosted/test/audit-probe.mjs` (+ `run-audit-probe.sh`): 4 accounts (alice, bob, carol, mallory) attacking each other against real workerd (`wrangler dev`: Durable Objects, D1, WebSockets) | 6 pass, 9 findings (all reproduced, see §1) |
| Live site, signed out: headers on `/paper/` and assets, `/paper/api/me`, `/pages`, cross-origin POST, bogus link token, `/admin/*` | all correct (below) |
| Live site, signed in with two throwaway accounts | **not done**: omegagiven.com only signs in with Google or GitHub, so a throwaway account needs a throwaway Google/GitHub account, which an agent can't make. The probes ran against the same code under real workerd instead (identical API/room code; only the auth adapter differs). |
| `npm audit` | omegagiven-com has no runtime deps (only `wrangler` as a dev dep, and no lockfile); og-paper web has no npm deps; `hosted/` has none. Nothing to audit beyond pdf.js (see M8). |

Re-run the probes any time: `forge run bash hosted/test/run-audit-probe.sh`.

**What's already good** (worth saying, because it's most of it): no IDOR anywhere
(a stranger gets 404 on every page, member, link, transfer and admin route and
can't open the room); viewers and editors are held to their roles server-side,
in the API *and* the room; revoking kicks live sockets at once; page ids are
128-bit and link tokens 192-bit (stored hashed); no public listing; every D1
query is parameterised (no injection); OAuth only (no passwords to leak), state
cookie HMAC-signed, session token random 256-bit stored as SHA-256, cookie
`HttpOnly; Secure; SameSite=Lax`, logout deletes the session server-side;
cross-origin writes refused (Origin + Sec-Fetch-Site); HSTS, `X-Frame-Options:
DENY`, `frame-ancestors 'none'`, `nosniff`, COOP, a CSP with `connect-src 'self'`
are live on `/paper/`; every user-supplied string I found in the web app
(page names, people, peers, toasts) goes through `esc()` before `innerHTML`;
quota can't be exceeded (single conditional UPDATE); per-socket and per-account
rate limits exist; the service worker never caches `/api/`; the privacy policy
is honest that the site holds page keys.

## Status (updated by #219, 2026-11-03)

Fixed in og-paper (see HOSTED.md "Safety nets"; tests in
`hosted/test/hardening.test.js` and `hardening-e2e.mjs`; the probes in
`audit-probe.mjs` now expect the fixes):

| # | Fix |
| --- | --- |
| H1 | Full copies keep what they replace for 30 days (owners/admin bring it back); a non-owner's copy under 10% of what it replaces replaces nothing |
| H2 | Give away is an offer the recipient accepts; by username/email only (raw ids refused) |
| H4 (part) | Delete goes to a 30-day trash (restore, delete for good). Off-site backups and the restore runbook are still to do |
| H5 | Pictures and PDF pages over the change limit are re-encoded and scaled down in the app before upload |
| M2 | Signing out (or another account) removes the account's links (keys) and offline copies from the device |
| M3 | Only owners/editors pin a room; owners may re-pin an empty one; `POST /pages {room}` pins at creation |
| M4 (part) | Removing/demoting a co-owner turns off the links they made; owners see who made each link. Co-owners can still remove each other; no event log yet |
| L1 | Writes need `Origin` or `Sec-Fetch-Site` |
| L3 | Page and people names cleaned server-side |

Still open: C1 and C2 (human: Workers Paid, Terms/Privacy v2), H3, H4
backups, H6, H7, M1, M5-M11, L2, L4-L9.

## Severity summary

| # | Sev | Issue | Area |
| --- | --- | --- | --- |
| C1 | **Critical** | Whole site runs on the Workers **Free plan**: one marketing spike exhausts the daily request/DO/row-write caps and OG Paper (and sign-in, games, Stripe webhooks) stop until 00:00 UTC | ops |
| C2 | **Critical (before paid)** | Terms of Service don't cover hosting user content or selling storage: no content licence, acceptable use, takedown/DMCA, refunds, age, governing law; only contact is GitHub | legal |
| H1 | High | An editor (or anyone with an edit link, or a buggy client) can **erase the server copy** of a page with an 8-byte checkpoint | data |
| H2 | High | **Give away** pushes a page into *any* account without consent (eats their quota, can plant content), even to account ids that don't exist | authz/abuse |
| H3 | High | OG Paper shares an **origin** with the whole site (games, chat, feedback, game submodules) under a CSP with `'unsafe-inline'`: an XSS anywhere on omegagiven.com can read every page key in localStorage and drive the paper API as the user | security |
| H4 | High | **No backups or restore path**: deleting a page is instant `deleteAll()`, no trash; no D1 export, no tested PITR | data |
| H5 | High | **Pictures/PDFs over 1 MB are refused** when signed in ("That change is too big to keep"): a normal phone photo fails | product |
| H6 | High | **Moderation isn't wired up**: reports go to `console.log` only, no alert, no admin UI, no abuse contact | abuse |
| H7 | High (before paid) | **Paid storage doesn't exist yet**: no `PAPER_UPGRADE_URL`, no subscriptions/entitlements/receipts/tax; only donation perks raise quota | business |
| M1 | Medium | No self-serve **account deletion** or **export of everything** (privacy policy says "contact us through GitHub") | privacy |
| M2 | Medium | **Signing out leaves** every opened page's link (incl. edit keys) in localStorage and its content in IndexedDB (shared computers) | privacy |
| M3 | Medium | A **viewer can brick a new page** by joining first with a bogus room id (room pinning) | authz |
| M4 | Medium | **Links outlive the co-owner** who made them; co-owners can remove each other; no audit trail of member changes | authz |
| M5 | Medium | Editors/edit-link holders can **burn the owner's whole quota** with junk; **viewers' passed-on frames reach editors** | abuse |
| M6 | Medium | **No monitoring/alerting/status page**: Worker logs only | ops |
| M7 | Medium | **Big pages**: the room loads a page's whole log into memory on join; fine at 25 MB, not at the 100 MB-1 GB you'd sell | perf |
| M8 | Medium | pdf.js loaded at runtime from cdnjs (whole cdnjs allow-listed in `script-src`, no SRI) | supply chain |
| M9 | Medium | Page keys travel in the WebSocket URL (`?k=`), so they land in Worker request logs (observability on) | privacy |
| M10 | Medium | Marketing copy risk: HOSTED.md says "sealed end to end… the room never reads a page", but the site stores every page's keys | claims |
| M11 | Medium | Invitees aren't told they were shared with (no email/notification); native apps can't sign in to the account | product |
| L1-L9 | Low | see §6 | |

---

## 1. Security

### H3 · Same origin as the whole site, `'unsafe-inline'` CSP — High

`/paper/` is served from `https://omegagiven.com`, the same origin as the
chat widget, the public feedback board, game lobbies with player names, and
third-party game builds under `/play/*`. The `/paper/*` CSP allows
`'unsafe-inline'` scripts (and all of cdnjs). Nothing I read is injectable today
(chat, feedback and account pages escape output), but any future XSS on *any*
page of the site can:

- read `localStorage['og-hosted-pages']`, which holds the relay link **with the
  edit key** for every page this browser opened, and IndexedDB copies of their
  content;
- call `/paper/api/*` with the user's cookie (same origin passes the Origin
  check): list, share to the attacker, make edit links, give pages away, delete.

**Fix (in order of value):**
1. Serve OG Paper from its own origin, `paper.omegagiven.com` (separate
   localStorage/IndexedDB and a CSP of its own); sign-in can stay on the
   apex with a short redirect handshake, or scope a second cookie to the
   subdomain.
2. Until then, replace `'unsafe-inline'` on `/paper/*` with a hash of the one
   inline module script in `web/app/index.html` (the meta tags the Worker
   injects aren't scripts) and drop `https://cdnjs.cloudflare.com` (see M8).
3. Clear `og-hosted-pages` and the account pages' IndexedDB copies on sign-out (M2).

### H2 · Give away without consent / to nobody — High

`POST /pages/:id/transfer` accepts `who` (username/email) **or a raw `to` id**
and moves the page immediately. Reproduced (probe 7): mallory made a 50 KB page,
gave it to bob with `leave: true`; it appeared in bob's *My pages* and counts
against bob's quota (bob used 50000 B). Repeating it fills anyone's 25 MB and
100-page cap so they can't make pages, or plants content in their account
that bob is now the billing owner of. Giving to `to: "no_such_user_x"` also
returns 200, leaving an orphaned page in storage nobody can reach or delete.

**Fix:** make it an offer: `paper_transfers(page_id, from, to, created_at)`;
the recipient sees "X wants to give you *page*" and accepts (bytes move then)
or declines. Resolve the target only through `auth.find()` (drop `to`, or
check it exists in `users`). Rate-limit offers.

### H1 · An editor can erase the server copy — High (data)

The room trusts any editor's checkpoint (`CKPT`) to stand for every frame
the sender had seen, and deletes them. It can't check what's inside (sealed).
Reproduced (probe 4): alice kept 5 changes, bob (editor) sent an 8-byte
`CKPT`; a viewer joining next received exactly one 8-byte frame. Anyone with an
**edit link** can do this, and so can a buggy client.

It heals if a device holding the full page reconnects (the app pushes what
the room lacks after `READY`), but a page last edited from a cleared browser,
or a new device, is gone, and there's no server-side history or backup (H4).

**Fix:** keep what a checkpoint replaces for N days instead of deleting it
(mark frames `superseded_by`, purge on the alarm after 7-30 days; they still
count to quota, or not, your choice), and add an admin "roll back to before
seq X" that un-supersedes. Cheap extra guard: refuse a whole-page checkpoint
much smaller than what it replaces (e.g. < 10% of covered bytes) unless it
comes from an owner.

### M3 · A viewer can pin a fresh page to the wrong room — Medium

The first `ROOM` id a room sees is pinned forever, and pinning happens before
the role check. Reproduced (probe 6): carol (viewer) joined a fresh page first
with a bogus id; alice (owner) was then refused with "This link's key does not
belong to this page". Small window (owner usually joins on creation), but it
bricks the page with no fix but deleting it.

**Fix:** only `EDIT` roles may pin; better, send the room id with
`POST /pages` (the browser makes the keys, so it knows it) and pin at creation.

### M4 · Links outlive their maker; co-owners remove each other — Medium

Reproduced (probes 9-10): a co-owner made an edit link, the billing owner
removed that co-owner, and the link still let mallory in as editor. Any
co-owner can also remove or demote other co-owners. Neither is wrong by
itself, but owners can't see who made which link, and nothing records who
changed what.

**Fix:** when a co-owner is removed or demoted, turn off the links they made
(or show "made by X" with one-tap revoke); add a small `paper_events` log
(who shared/removed/linked/transferred, when) visible to owners.

### M5 · Quota burning and viewer frames — Medium

- An editor (and anyone holding an edit link) can send junk `PUT`s up to the
  owner's full quota: the room can't tell junk from strokes. Per-socket limits
  are 512 KB/s, so 25 MB takes under a minute. **Fix:** per-editor share of
  the owner's quota (e.g. non-owners may add at most 25% per day), and an
  owner-visible "who used what" from the `frames.author` column.
- Probe 5: a **viewer's** `EPH` frames (presence/strokes-in-progress) are
  passed to everyone. They're sealed, and the app should drop what it can't
  verify, but it's free bandwidth and noise from read-only people.
  **Fix:** lower EPH rate for viewers, or pass only presence-sized frames.

### CSRF, session, auth — mostly fine

- Writes need same-origin `Origin`/`Sec-Fetch-Site` **when present**; a POST
  with neither still succeeds (probe 8). Every current browser sends `Origin`
  on cross-site POSTs and the cookie is `SameSite=Lax`, so this is **Low**.
  Fix: require one of the two headers on non-GET (`fail(403)` when both absent).
- Sessions: 60 days, no rotation, no "sign out everywhere", no list of active
  sessions. **Low.** Add both to the account page.
- No password reset / email verification needed (OAuth only). GitHub may give
  no email, which is fine because sharing by email only matches emails a
  provider reported.
- Signing in with a second provider while signed in links it to the current
  account (intended). Fine with the HMAC'd state.

### Stored XSS — no hole found, defence in depth (Low)

Page content is drawn by the Rust/wasm renderer onto a `<canvas>`, never as
HTML. Page names, people's names, peer names, toasts and server error messages
are escaped (`esc`/`fmt`) everywhere I found an `innerHTML`. Probe 3: the API
stores names verbatim (`<img src=x onerror=…>` accepted), so any future
renderer (an email, an admin page, the account page on omegagiven.com listing
pages) must escape too. **Fix (Low):** strip `<>` and control characters from
names server-side as a second net.

### M8 · pdf.js from cdnjs — Medium

`og-web.js` imports `pdf.js/4.10.38` from cdnjs at runtime (dynamic `import()`
can't carry SRI), and `script-src`/`worker-src` allow all of
`https://cdnjs.cloudflare.com`, which hosts libraries usable to bypass a CSP.
4.10.38 itself is past the 2024 font-eval CVE (CVE-2024-4367, fixed in 4.2.67).
**Fix:** ship pdf.js inside `web/app/vendor/` (it's ~1.5 MB, loaded only on
first PDF import) and drop cdnjs from the CSP.

### M9 · Keys in the WebSocket URL — Medium

`openInfo()` returns `wss://…/relay?k=<edit or view key>&c=<canvas>`. The
server doesn't need `k` (it checks membership), but with `observability`
on, request URLs (with query strings) go to Worker logs. **Fix:** strip the
query before the request reaches anything that logs, or have the app put the
key only in its local link and dial the bare path.

### Rate limits and abuse

- API: per user (or IP) 240 GET / 30 writes / 8 creates / 40 sockets a minute;
  room: 60 frames/s per socket, 32 people per room. Adequate for launch.
- Sign-up: any Google/GitHub account gets 25 MB and 100 pages; nothing caps
  accounts per day. **Low-Medium**: consider a smaller quota for the first
  days of an account, or OG Paper only after picking a username (already
  required in practice).
- Phishing on omegagiven.com: lower risk than a document host, because pages
  aren't HTML and links need sign-in by default (`linkAnonymousView` off).
  Keep anonymous links **off** at launch; if you turn them on, add a
  "user content, not from OmegaGiven" banner for visitors.
- Malware uploads: pictures/PDFs travel sealed inside changes and are only
  ever drawn to a canvas (PDF rendered by pdf.js in the browser), never served
  as files. Low.

---

## 2. Data safety

### H4 · Backups and restore — High

- Content lives in one SQLite Durable Object per page; the directory in D1.
  Cloudflare gives D1 **Time Travel** (point-in-time restore) and SQLite DOs
  **point-in-time recovery** (`getBookmarkForTime` /
  `onNextSessionRestoreBookmark`), but nothing in the code or runbooks uses
  either, and neither covers "I deleted my page yesterday" in a way the user
  can reach.
- `DELETE /pages/:id` calls `storage.deleteAll()` and deletes the D1 rows at
  once. No trash, no undo. A co-owner can't delete, but the billing owner (or a
  hijacked session) can wipe everything in seconds.

**Fix:**
1. **Trash:** `paper_pages.deleted_at`; delete hides the page and kicks
   sockets; an hourly cron (one already exists) purges after 30 days; "Restore"
   in My account. Bytes can stay counted until purged.
2. **Off-site backup:** nightly `wrangler d1 export omegagiven-accounts` to the
   NAS (from og-host1), plus a DO "export" endpoint for admins that streams a
   page's frames, run nightly for pages changed that day.
3. **Restore runbook**, tested once on a scratch page: D1 Time Travel
   restore, DO PITR bookmark restore, and from the NAS export.

### Concurrent edits

Good: the merge model is add-only with HLC stamps (tested in
`collab.test.js`, the workerd smoke test, and probe 4's recovery path);
offline edits merge on reconnect; the app warns before closing with
unsaved work. The only loss path found is H1.

### Version history

The app's timeline (every change, scrub back in time) is a real selling
point, but on the server a checkpoint replaces history once a page's backlog
passes 1 MB (whatever history the checkpoint carries is all there is).
H1's "keep superseded frames N days" also gives server-side history.

### At quota

Works as documented: changes are refused, cards at 80/95/100%, nothing is
deleted, work stays in the browser and goes up when there's room. When a
donation perk expires, the hourly cron lowers the quota below `used`: edits
then stop until the user frees space, with no warning. **Low:** send a
notice before a perk expires and give a grace period.

### M1 · Account deletion and export — Medium (High if you sell to EU/CA users)

No self-serve account deletion anywhere on the site; privacy page says to
ask through GitHub. There's per-page Export in the app, but no "download all
my pages". There's also no cleanup of OG Paper data if an account row were
deleted by hand (pages, members, links, DOs, perks).

**Fix:** "Delete my account" on `/account/`: delete owned pages (rooms
`destroy`), memberships, links, reports by the user, sessions, identities,
the user row, `paper_accounts`; confirm with the username typed. "Export all
pages": a zip of `.ogpt` files built in the browser from each page. Give an
email address for requests.

### M2 · Sign-out leaves pages on the device — Medium

`og-hosted-pages` in localStorage keeps each opened page's link (with the
**edit key** for editors) and IndexedDB keeps its content, after signing out.
On a shared/library computer the next person can reopen them offline.
**Fix:** when `/me` says signed out (or the account changed), drop those
entries and their IndexedDB copies; or ask "Keep offline copies on this
device?" at sign-in.

---

## 3. Production readiness

### C1 · Free plan limits — Critical before marketing

`README.md` in omegagiven-com: "Uses the Workers free plan". Daily caps,
shared with **every** part of the site:

| Free cap / day | OG Paper use |
| --- | --- |
| 100,000 Worker requests | every API call, page load of `/paper/`, every socket |
| 100,000 DO requests | ~1.35/s per person drawing → ~20 pen-hours a day across the site |
| 100,000 DO rows written | ~1 per kept stroke → ~90,000 strokes a day |

A Hacker News/Reddit post easily passes these by midday; then sign-in,
saving, games and Stripe webhooks all fail until 00:00 UTC, with users seeing
"not saved" cards. **Fix:** Workers Paid ($5/month) before any marketing; turn
on Cloudflare billing/usage notifications; the cost table in `HOSTED.md`
shows paid usage stays in cents-to-dollars at hundreds of users.

### M6 · Monitoring and alerting — Medium

Only Worker logs (`observability.enabled`). No uptime probe, no error alert,
no client error reporting, no status page; reports and 500s are seen only if
someone reads logs.

**Fix:** an og-host1 probe (og-watcher style) every 5 min: `GET
/paper/api/me`, load `/paper/`, open a socket to a probe page; Signal on
failure. Cloudflare notifications for Worker error rate and usage. A
`status.omegagiven.com` (or a simple `/status` page fed by the probe).
Logging: today logs hold user ids and, via URLs, page keys (M9); no emails
or names logged in paper code. Keep it that way.

### M7 · Performance on big pages — Medium (blocker for bigger paid quotas)

- `backlog()` reads `SELECT … FROM frames WHERE seq > ?` with `.toArray()`:
  the whole page log in DO memory (128 MB limit) on every join; the Rust app
  then loads the page whole (tiles aren't used by the app yet, per HOSTED.md).
- At 25 MB this is fine; at 250 MB-1 GB plans it will time out or crash.
- **Fix before selling >100 MB:** stream the backlog in pages of rows, make
  the app subscribe by tiles, store old content in R2 (not bound today).
- `/paper/` loads ~4.5 MB of wasm (compressed transfer measured 0.55 s
  here). OK for desktop; slow on 3G. Consider a loading progress bar.

### Mobile, accessibility, browsers

- Viewport has `maximum-scale=1, user-scalable=no` (blocks page zoom for
  low-vision users in the panels; the canvas zooms itself). **Low:** drop it for
  panels, or keep and make panel text sizes adjustable.
- Panels are HTML (buttons, lists), mostly labelled; the canvas isn't
  screen-reader accessible (normal for drawing apps; say so in an
  accessibility note).
- Needs WebAssembly, ES modules, WebSockets, IndexedDB: Safari 15+, Chrome/Edge/Firefox current. Add a "your browser is too old" message if wasm fails to load.

### Limits per account

25 MB, 100 pages, 25 people a page, 5 links a page, 32 people live in one
room, 1 MB per change. All server-enforced. Fine for free; see H5 for 1 MB.

---

## 4. Legal and business (before marketing and paid plans)

### C2 · Terms of Service — Critical before paid, High before marketing

`/terms` (Oct 18) covers games and "paid services billed through Stripe" in
two lines. Missing for a service that hosts people's content and takes money:

- **User content:** you keep ownership; you grant OmegaGiven a licence only to
  store, transmit and display it to the people you share with.
- **Acceptable use:** no illegal content, CSAM, harassment, malware, phishing,
  copyright infringement; right to remove and suspend.
- **Takedown / DMCA:** a designated agent (register at copyright.gov, $6) and
  an `abuse@`/`dmca@` email; repeat-infringer policy. Without a registered
  agent there's no safe harbour for user uploads.
- **Paid plans:** what's included, renewal, cancellation, **refund policy**,
  price changes with notice, what happens to data over quota after a
  downgrade (read-only, kept N days, then?).
- **Data:** no guarantee, backups as described, export available, deletion on
  account close after N days.
- **Age:** 13+ (16+ in the EU without parental consent); privacy says "not
  directed at children under 13" but Terms don't set an age.
- Governing law/venue, limitation of liability that matches paid service,
  termination, a real **contact email** (today only "github.com/OmegaGiven").

### Privacy policy — mostly good, a few gaps (Medium)

Good: names the data, Cloudflare/Stripe, and is honest that the site holds
keys. Add: a contact email; retention periods (sessions 60 days, pages until
deleted, trash 30 days if H4 is done, logs); self-serve deletion/export once
M1 ships; the legal basis/rights for EU/UK users if you sell there; a line
that only strictly necessary cookies are used (session, OAuth state), which
also means **no cookie banner is needed** (Cloudflare Web Analytics is
cookieless) as long as that stays true.

### H7 · What paid storage needs — High (before taking money)

Today: `PAPER_UPGRADE_URL` isn't set (so "Get more storage" never shows);
quota only moves via donation perks (`donations.js`, hourly cron); no plans.

Needed:
1. **Stripe Billing**: Products/Prices for tiers (e.g. 1 GB / 10 GB, monthly
   and yearly), Checkout in subscription mode, **Customer Portal** (cancel,
   change card, invoices = receipts).
2. **Entitlements**: webhook (`customer.subscription.created/updated/deleted`,
   `invoice.paid`, `invoice.payment_failed`) → `paper_accounts.quota` (reuse
   the perk mechanism: a `subscription` perk without expiry, removed on
   cancel); grace period on failed payment; downgrade = read-only over quota,
   never delete.
3. **Tax**: Stripe Tax (or a merchant of record like Paddle/Lemon Squeezy, which
   handles VAT/sales tax for you; simpler for a one-person shop).
4. A **pricing page** and the upgrade URL; plan shown in My account.
5. Business basics: business entity/bank account for payouts, support email,
   refund policy (above).
6. Before selling bigger quotas: M7 (big pages) and R2 bound.

### Support and status

No support email or form for OG Paper (the feedback board is public: not for
account problems). Add `support@omegagiven.com` and link it from My account.
Status page: see M6.

---

## 5. Product gaps (what users will expect)

### H5 · Pictures and PDFs over 1 MB fail when signed in — High

`maxOpBytes` is 1 MB and the app doesn't downscale. A phone photo (2-6 MB) or
a typical PDF dropped on an account page is refused with "That change is too
big to keep" (and the "not saved" chip). This will be one of the first things
a new user tries. **Fix:** downscale pictures in the app before sending
(longest side 2048-3072 px, JPEG/WebP ~0.85), rasterise PDF pages at a sane
DPI, and show a friendly message when something is still too big. (Raising
`maxOpBytes` is the stop-gap: the cap can go to ~30 MB.)

### M11 · Sharing and accounts UX — Medium

- Invitees aren't told: sharing by username just adds the page under their
  *Shared with me*. Send an email (or a site notification) "alice shared *page*
  with you" with a link.
- Native apps (desktop, iOS, Android) don't sign in to the account; the privacy
  page even says they don't connect to omegagiven.com. "Pay for hosting your
  pages" sells much better as "your pages on every device, including the
  apps". Today an app can join a page only by pasting its relay link.
- No search across pages (names only), no folders/tags, no templates; fine for
  launch, expected within months.
- No landing page for hosted OG Paper: `/paper/` drops straight into the app
  with no meta description / Open Graph image, so links shared on social show
  a bare title. Add a short "what is this" first-run card and OG tags.

### M10 · Claims to avoid in marketing — Medium

`docs/HOSTED.md` says frames are "sealed end to end" and "the room never reads
a page". True of the room code, but the D1 directory stores every page's keys
and hands them to members, so OmegaGiven (and anyone who compromises the
Worker or the Cloudflare account) can read pages. Don't market "end-to-end
encrypted". Accurate: "encrypted in transit, private to the people you share
with; we never look at your pages except to act on an abuse report". (The
privacy policy already says this correctly.) True E2E would need keys
wrapped per user (e.g. a passphrase or device keys) and loses admin review.

### Would embarrass a launch

1. Free-plan outage on launch day (C1).
2. "Too big to keep" on the first photo (H5).
3. A troll with an edit link wiping a shared page (H1) or dumping pages into
   accounts (H2).
4. A takedown request with no abuse contact (C2/H6).

---

## 6. Low

| # | Issue | Fix |
| --- | --- | --- |
| L1 | Writes with neither `Origin` nor `Sec-Fetch-Site` pass the CSRF check | `fail(403)` when both are absent on non-GET |
| L2 | No "sign out everywhere" / session list; 60-day sessions | add both to `/account/` |
| L3 | Page names stored with raw HTML (client escapes) | strip `<>` and control chars on write |
| L4 | Quota lease drift if a room is evicted mid-lease (≤ 256 KB per room) | hourly recount of accounts with busy rooms, or recount on `/me` once a day |
| L5 | Perk expiry drops quota under `used` with no warning | notice 7 days before; grace period |
| L6 | HSTS without `includeSubDomains; preload` | add once every subdomain is HTTPS-only |
| L7 | `user-scalable=no` viewport | allow zoom outside the canvas |
| L8 | omegagiven-com has no `package-lock.json` (wrangler resolved fresh each install) | commit a lockfile |
| L9 | Blocked page owners still see blocked pages listed (no reason shown) | show "taken down: contact support" |

## 7. H6 · Moderation — High (detail)

- `onReport` in omegagiven-com is `console.log`: nobody is told. **Fix:** send it
  to Signal through the existing relay (or email) with the page id and reason.
- Admin actions exist only as API routes (`/paper/api/admin/*`); there's no
  page to list reports, look at a page, take down, or block. **Fix:** a small
  `/paper/admin/` page (reports list, open view-only, take down, block account,
  set quota).
- Check `PAPER_ADMINS` is set as a secret (couldn't verify from here).
- Publish `abuse@` and the DMCA agent (C2).

---

## Ready-to-do follow-ups (small, safe)

These are small, self-contained and covered by the existing tests; none were
done in this task:

1. Transfer: resolve the target only via `auth.find`, refuse unknown ids (part of H2). ~10 lines in `api.js` + a test.
2. Room: only `owner`/`editor` sockets may pin the room id (M3). 1 line in `room.js` + a test.
3. API: require `Origin` or `Sec-Fetch-Site` on non-GET (L1). 1 line.
4. Strip `<>`/control chars from page names server-side (L3). 1 line.
5. Sign-out cleanup of `og-hosted-pages` + IndexedDB copies (M2). ~20 lines in `og-hosted.js`.
6. Removing/demoting a co-owner turns off their links (M4). ~5 lines.
7. omegagiven-com: `onReport` → Signal relay; add OG/meta tags to `/paper/`; set `PAPER_UPGRADE_URL` once a page exists.

## Follow-up tasks I'd dispatch

| Order | Task | Repo | Covers |
| --- | --- | --- | --- |
| 1 | Move omegagiven.com to Workers Paid + usage/error notifications (human: billing) | omegagiven-com / Cloudflare | C1 |
| 2 | Terms v2 + privacy v2 + abuse/support email + DMCA agent (human: legal choices) | omegagiven-com | C2, H6 |
| 3 | Hardening batch: follow-ups 1-6 above + give-away as offer/accept | og-paper | H2, M2-M4, L1, L3 |
| 4 | Checkpoints keep superseded frames 30 days + admin rollback; page trash 30 days | og-paper | H1, H4 |
| 5 | Downscale pictures / rasterise PDFs before sending; friendly too-big message | og-paper | H5 |
| 6 | Nightly D1 export + page export to the NAS; tested restore runbook | omegagiven-com + ops | H4 |
| 7 | Reports → Signal; `/paper/admin/` page | omegagiven-com + og-paper | H6 |
| 8 | Delete account + export all pages | omegagiven-com + og-paper | M1 |
| 9 | Uptime probe on og-host1 + status page | new / og-watcher | M6 |
| 10 | Own origin `paper.omegagiven.com`, hash-based CSP, self-hosted pdf.js, key out of WS URL | omegagiven-com + og-paper | H3, M8, M9 |
| 11 | Stripe subscriptions → quota, pricing page, customer portal, tax | omegagiven-com | H7 |
| 12 | Streamed backlog + tiles in the app + R2, before plans over 100 MB | og-paper | M7 |
| 13 | Share notifications; native apps sign in to the account; landing/first-run | og-paper + omegagiven-com | M11 |

## Before-marketing checklist

- [ ] Workers Paid on, usage alerts on (C1)
- [ ] Terms v2 (content, AUP, takedown, age, refunds), privacy v2, support + abuse email, DMCA agent (C2)
- [ ] Give-away needs the recipient to accept (H2)
- [ ] Editors can't erase the server copy; page trash (H1, H4)
- [ ] Photos/PDFs over 1 MB work (H5)
- [ ] Reports reach you on Signal; you can take a page down from a page, not curl (H6)
- [ ] Nightly backups off Cloudflare, restore tested once (H4)
- [ ] Uptime probe + status page (M6)
- [ ] Sign-out clears offline copies; delete-account + export-all (M1, M2)
- [ ] `/paper/` has OG tags and a first-run card; marketing says "private", not "end-to-end encrypted" (M10, M11)

Before taking money, also: Stripe subscriptions + portal + tax, pricing page,
`PAPER_UPGRADE_URL`, refund policy, big-page work if any plan is over 100 MB
(H7, M7).
