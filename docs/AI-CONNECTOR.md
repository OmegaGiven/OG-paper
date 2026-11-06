# OG Paper AI connector: design

Task #352, 2026-11-06. Design only: nothing here is built, deployed or
submitted. Docs were checked on this date; section 2 marks what could not be
confirmed.

## 1. Summary

One remote MCP server at `https://ogstudio.org/paper/mcp`, with OAuth against
the OG Studio account, serves Claude, ChatGPT and other MCP clients. Four
things found while designing change the plan:

1. **A Worker cannot read a hosted page.** Rooms hold sealed frames. The keys
   are in D1 (`paper_pages.edit_key/view_key`), but the code that understands
   a page (`script.rs`, on `App`) needs the Rust engine, and `view_page`
   pictures are native-only (`script.rs` `picture`, wasm returns an error).
   So the connector needs a **native engine peer** (section 4).
2. **Chat AIs are turn-based.** Nothing found says Claude or ChatGPT wake the
   model when a page changes. MCP's `subscriptions/listen` reaches the client,
   not necessarily the model. The AI sees changes on its next turn. Only an
   in-app assistant can react the moment you draw (sections 6 and 8).
3. **Undo for AI edits does not exist yet.** `run_commands` records the edit
   in the *server's* `App` history. A person's undo is local to their own app,
   and no MCP tool undoes. So `docs/MCP.md` ("can be undone in the app") looks
   wrong for hosted use; the timeline can restore instead. This is from
   reading the code, not from running it.
4. **Native apps cannot sign in to OG Studio yet.** The room route needs a
   site session (`need()` in `hosted/src/api.js`); the app's `SignIn` is only
   a page-server password. Native apps need an account sign-in first.

Two blockers for directory review: **the site signs in only with Google or
GitHub**, and OpenAI's reviewers need a login with no MFA, email codes or
magic links, so a reviewer login is needed. And **the privacy policy must
change**, because page content would be decrypted in a server process and
sent to the user's AI provider.

## 2. What each client requires today

| | Claude | ChatGPT |
| --- | --- | --- |
| Without a listing | Custom connector by URL, on all plans (Free: one). Prefilled install link possible | Developer mode. Plans: **unconfirmed** (help page returned 403; search snippets say Business/Enterprise/Edu for write actions, and sources conflict on Plus/Pro) |
| Listing | Portal at claude.ai/directory/manage, any paid plan. Automated scan, then listed as **Community** by default; Verified is by escalation, no application | Dashboard submission. Org owner or Apps Management Write, individual or business verification. Docs now call these **plugins**. Review, then you choose when to publish |
| Client registration | CIMD if metadata has `client_id_metadata_document_supported: true` **and** `none` in `token_endpoint_auth_methods_supported`; otherwise DCR. Anthropic-held client also possible | CIMD preferred, DCR, or registered client |
| Redirects | `https://claude.ai/api/mcp/auth_callback`, plus loopback on any port for `localhost` and `127.0.0.1` (Claude Code) | Exact URI from the management page; `https://chatgpt.com/connector_platform_oauth_redirect` with RFC 9207 |
| OAuth details | 401 with `resource_metadata`; PRM `resource` equals the URL exactly; first `authorization_servers` entry only; S256; form-encoded `/token`; rotate refresh tokens; `invalid_grant`; 10 s limit (30 s refresh) | S256 required; `resource` on both requests, copied to audience; per-tool `securitySchemes`; `_meta["mcp/www_authenticate"]` |
| Tools | `title` plus `readOnlyHint` or `destructiveHint`; names up to 64 chars; read and write separate; no behaviour steering in descriptions; useful errors; sized responses | All three hints as explicit booleans. Being undoable does **not** justify `destructiveHint: false`. Each operation its own tool; retry-safe |
| Listing assets | Docs URL, privacy URL, support contact, icon, populated test account, 7 compliance acknowledgments; MCP App: 3-5 PNGs, 1000 px or wider | Website, support, privacy, **terms** URLs; icon; domain token at `/.well-known/openai-apps-challenge`; test account **without MFA/email/SMS/magic link**; 5 positive and 3 negative test cases; **video walkthrough**; release notes |
| Rules that bite | Domain should match the service; first-party API only; no AI image generation (diagrams are fine) | **No upgrade prompts or subscription offers inside the app**; suits ages 13+; daily rescans, new tools held until approved |

**MCP spec:** current revision 2026-07-28 removes sessions, `initialize`, the
GET stream and SSE resume; adds `server/discover`, `resultType`, `ttlMs` and
`cacheScope`, required `Mcp-Method`/`Mcp-Name` headers, and
`subscriptions/listen`. It deprecates DCR, sampling and roots. Serve both
2026-07-28 and 2025-11-25, since `mcp.rs` stops at 2025-11-25 and client
adoption of the new revision was not confirmed.

**MCP Apps** (inline UI, `io.modelcontextprotocol/ui`): supported by Claude,
Claude Desktop, VS Code Copilot, M365 Copilot and others. OpenAI's docs mention
it, but the page with the details could not be read, and the MCP overview does
not list ChatGPT. Treat ChatGPT support as **unconfirmed**.

**Not confirmed today** (fetch limit reached): Gemini, Le Chat and Copilot
consumer connector details (third-party listings only), Cloudflare Containers
prices (from memory), Apple 3.1.1 for AI credits. Apple 5.1.2(i), explicit
permission before sharing with third-party AI, is from secondary sources.
Recheck all of these before relying on them.

## 3. Architecture

```
Claude / ChatGPT / any MCP client
        | HTTPS, Bearer token
ogstudio.org Worker (hosted/src/mcp.js + oauth.js)
   token check, scopes, page grants, rate limit, audit
        |                            \
PaperEngine (Container, native)     D1: grants, tokens, audit
   og-paper --engine: the same script.rs and mcp tools
        | joins as a relay peer, name "Claude (via Sam)"
PaperRoom (Durable Object, unchanged protocol)
        |
web and native apps: see edits live, with the AI's cursor
```

Lives in `hosted/` plus one new mode in the Rust binary, off by default and
never compiled into App Store targets (section 12).

## 4. Reading and editing pages: the options

| Option | Good | Bad |
| --- | --- | --- |
| **A. Native engine peer in a Container** (recommended) | Reuses `hub.rs`, `script.rs`, `mcp.rs` and pictures. Works with no app open. View key enforces read-only by cryptography | Needs Containers; decrypts server-side |
| B. Wasm in a Durable Object | No containers | `script.rs` needs splitting from `App`; no pictures; 128 MB limit against a 12.3 MB wasm plus pages up to 25 MB |
| C. Only an open app runs tools | Keys never leave devices; sees view and selection | Nothing works when the app is closed; iPad suspends it |
| D. og-host2 as the engine | Free, good for dev | Public service on a home node with a nearly full pool, needs public ingress, holds decrypted pages |

**Choice: A in production, C added for local pages (section 7), D for
development only**, on og-host2 as an OG-Host app behind `tailscale serve`.
claude.ai cannot reach the tailnet, so dev testing uses MCP Inspector and
Claude Code; a real claude.ai test needs a public URL and your approval.

One engine holds many pages and unloads idle ones, as `hub.rs` does (`IDLE`).
It gets a page key per call and keeps it in memory only. A token with read
access loads the **view key**, so it cannot sign edits any app would accept.

## 5. Tools

Existing tools carry over; `edit_page` is split because both directories want
one operation per tool.

| Tool | Scope | Hints |
| --- | --- | --- |
| `list_pages`, `read_page` (paged), `view_page`, `list_fonts` | `pages:read` | readOnly |
| `get_focus`: what the person is looking at and has selected | `pages:read` | readOnly |
| `get_changes`: what changed since a cursor, and by whom | `pages:read` | readOnly |
| `get_inbox`: things the person sent to the AI | `pages:read` | readOnly |
| `add_to_page` | `pages:write` | not destructive |
| `update_items` (move, restyle, group) | `pages:write` | destructive |
| `delete_items` | `pages:write` | destructive |
| `undo_ai_edit` (by edit id) | `pages:write` | destructive |
| `create_page` | `pages:manage` | not destructive |
| `rename_page`, `trash_page` | `pages:manage` | destructive |

Deliberately **no** tools for sharing, members, links, transfers, exports to a
URL, chat, or anything reaching outside the account. Every result carries
`rev`, so the cursor is an explicit handle, as the stateless spec asks.
`?k=` addresses stay for self-hosted servers only; the spec bans them here.

## 6. Live state, both ways

**AI to person:** instant. Edits arrive as normal relay frames, with a named
peer, a cursor and a "Claude is editing" chip.

**Person to AI:** on the next turn, never by push.
- Each result includes `rev` and a short "changed since you last looked".
- `get_focus` needs apps to publish view and selection. Presence frames already
  carry the view; selection is new, and sent only while AI access is on.
- **Send to AI:** select things, tap it; they land in `get_inbox`, and the app
  offers to open Claude or ChatGPT. The person still has to say something there.
- Later, an MCP App widget in chat shows the live page and uses the model
  context update to report changes (Claude first; ChatGPT unconfirmed).
- Implement `subscriptions/listen` only if a client is shown to use it.

## 7. Native apps

- **P2 prerequisite:** account sign-in on native (system browser, PKCE, app
  token), and a bearer path to the relay route. Then hosted pages behave the
  same on every platform.
- **Local pages** are invisible to the connector. Two answers: move the page to
  the account, or **Share this page with AI**, where the open app holds an
  outbound socket and runs tools itself (option C). Nothing is stored; it stops
  when the app closes or is backgrounded on iOS.
- Claude Code keeps working with the local server. Local bundles are no longer
  accepted in Claude's directory (plugins only).
- The chat panel is web-only (`og-chat.js`), so native needs an egui panel.

## 8. Talking to the AI from inside the app

| Way | Cost to us | Notes |
| --- | --- | --- |
| Handoff: open Claude or ChatGPT with the page | none | P2, simple, not a real in-app chat |
| **Bring your own API key** | none | P4 first step; key kept on device. Explicit consent per provider |
| Hosted AI as a paid add-on | tokens | Waits for billing, which is deferred. Apple IAP rules unconfirmed |
| Sign in with a Claude subscription | n/a | **Not allowed**: Anthropic's terms bar third-party apps from consumer OAuth (news sources, check live terms) |

Input: type in a panel; dictate with the OS keyboard (free everywhere) or Web
Speech; or **write to it with the pen**, using the Write tool's existing
handwriting reader. That last one is the feature no other AI app has. The
assistant reacts to changes, shows as a peer in the page chat, and uses the
same tools and audit log with its own client id. API calls can use the MCP
connector or the same tool list directly.

## 9. Security

**Access = scope AND page grant AND the person's role AND the page's AI setting.**
The most restrictive wins. Security here must hold even if the model is fooled.

- **OAuth:** `hosted/src/oauth.js` on the site's session, through the auth
  adapter. CIMD with an allowlist for claude.ai and chatgpt.com; other URLs
  fetched with SSRF guards, size and time limits. DCR as a rate-limited
  fallback. S256 only, exact redirects, `iss` on all responses, audience checked.
- **Tokens:** opaque, stored as SHA-256 in D1 so revocation is immediate. Access
  1 hour; refresh rotated with reuse detection that kills the grant.
- **Consent screen:** shows the client's name and redirect host. Read-only is
  preselected; edit and manage are unticked. Choose all pages or chosen pages.
  The grant may be narrower than requested. Test whether Claude and ChatGPT
  step up on `403 insufficient_scope`; the consent screen does not depend on it.
- **Shared pages:** other people's work goes to your AI provider. Owners get a
  per-page setting like chat: AI off, read, or edit. Default: on for pages you
  solely own, **off** on pages with other members.
- **Revocation:** My account > Connected AI lists each connection with last use.
  Disconnect kills tokens and kicks the peer, using the existing `/kick`.
  Removing someone from a page ends their AI's access at once.
- **Audit log:** `paper_ai_audit`: time, user, client, tool, page, result, edit
  id, item counts. No page content or arguments. 90 days. Shown to the user;
  owners see AI edits on their pages.
- **Rate limits** via `auth.limit` and new per-grant buckets. Starting points:
  reads 60/min, writes 20/min, `view_page` 10/min, 500 items per call, 5,000
  writes a day. AI writes count against the owner's quota. Global cap on engines.
- **Undo:** each call gets an edit id and a dedicated peer id. Because items are
  never edited in place, undo is new show/hide events, which merge safely and
  reach every device. Show recent AI edits in the app with one-tap undo. A later
  human edit on the same item wins, and undo says what it skipped. Before a big
  change, the engine sends a checkpoint; `trash_page` keeps the 30-day restore.
  Note the signing key is shared per page, so authorship is what the service
  records, not something cryptographically proved.

**Prompt injection: page content is untrusted.** Anyone with edit access can
write to a page, and pictures can carry text. Specific to OG Paper: **endless
zoom hides text from people.** Instructions at 10^-40 scale, off-screen, or
white on white are invisible to a person and plain to `read_page`.

- Page text is returned as data, in structured fields, with its author, never
  merged into server instructions.
- `read_page` defaults to what is readable at the person's view; tiny, hidden
  or low-contrast text is flagged `hidden: true`, and left out unless asked for.
- No exfiltration tools (section 5), and no live links built from page content.
  Other connectors in the same chat stay a risk we cannot remove, so the docs
  will say so.
- Destructive tools are annotated so hosts confirm them. Server limits apply
  anyway: at most 200 deletions per call, and writes on a page with others
  present show a visible banner.
- Keep tool descriptions factual; both directories scan them. `INSTRUCTIONS` in
  `mcp.rs` needs trimming to how the canvas works.
- Test suite: a page of attack strings, checked as P1 and P2 release gates.

## 10. Cost to run

The user's own Claude or ChatGPT plan pays for the AI. We pay for hosting.
Container rate is from memory: about $0.03 per instance-hour at most, for
1 GiB and 1/4 vCPU. Assumes 20 AI sessions of 10 minutes per user a month,
30 calls each, 10 sessions sharing an instance. All are estimates.

| Monthly users | Calls | Engine hours | Estimated total |
| --- | --- | --- | --- |
| 50 | 30k | 17 | about $5 (the Workers Paid minimum) |
| 1,000 | 600k | 333 | about $15 |
| 10,000 | 6M | 3,333 | about $110 |

Workers, DO and D1 stay inside included amounts at these levels, apart from
about $1 of DO requests at 10,000. Containers need Workers Paid; confirm the
site's plan. One-off costs: legal review of privacy and terms, the demo video.

**Hosted in-app AI**, if ever offered, costs far more. Rough cost of one
5-step request (5k cached and 5k fresh input, 800 output tokens per step):

| Model | Per step | Per request | 100 requests a month |
| --- | --- | --- | --- |
| Opus 5.5 | $0.04 | $0.20 | $20 |
| Sonnet 5.5 | $0.02 | $0.10 | $10 |
| Haiku 5.5 | $0.001 | $0.005 | $0.50 |

That is why BYOK comes first.

## 11. Phases

Sizes are working estimates for agent-built work, including tests.

| Phase | What | Size | Needs you |
| --- | --- | --- | --- |
| **P0** Local groundwork | Fix undo with edit ids, fix `MCP.md`, split `edit_page`, add hints, paging, hidden-text flags, 2026-07-28 support | M, 3-5 days | Review |
| **P1** Read-only connector | OAuth, consent, Connected AI page, audit, limits, engine with view keys, read tools. On og-host2, then private | L, 2 weeks | Containers, deploy, privacy text |
| **P2** Edit | Write tools, page AI setting, AI cursor and undo list, focus and inbox, handoff | L, 2 weeks | Test on claude.ai |
| **P3** Listings | Reviewer login, docs, terms, video, test cases, submit to Claude then ChatGPT | M, 1 week plus review | All submissions, verification, legal |
| **P4** In-app assistant | Web panel, BYOK, pen and voice input | L, 2 weeks | Provider choices |
| **P5** Native | Account sign-in, egui panel, Share page with AI | XL, 3-4 weeks | App Store review, consent wording |
| **P6** Extras | MCP App widget, suggest mode, hosted AI with billing | L each | Billing decision |

Ship P0 and P1 first: read-only proves OAuth, the engine and both clients with
the least risk. P5 starts only after the Mac resubmission is settled.

## 12. App Store safety

Nothing here touches that work: this is one document, on a branch. When code
starts, the engine goes behind a Cargo feature that iOS and Mac builds leave
off, and native AI work waits for P5. Any app that sends pages to an AI needs
explicit, unticked-by-default consent naming the provider, plus updated privacy
labels.

## 13. Decisions needed

1. **Production engine on Cloudflare Containers?** Recommended. It goes beyond
   the usual og-host2 rule, because public decrypted pages should not sit on a
   home node.
2. **Accept server-side decryption for AI?** The site already holds the keys,
   but this is a new use. It also rules out true end-to-end encryption for
   pages with AI on. Recommended: yes, per page, disclosed in the policy.
3. **Default for shared pages:** off, as proposed?
4. **Reviewer login:** add a password login limited to one test account?
   Recommended, with a seeded account and sample pages.
5. **Address:** `ogstudio.org/paper/mcp` needs no DNS change;
   `mcp.ogstudio.org` is cleaner but needs a record. Recommended: the path.
6. **Publisher name** for ChatGPT verification: OG Studio as a business, or
   individual?

## Sources

- Claude: [submission](https://claude.com/docs/connectors/building/submission),
  [authentication](https://claude.com/docs/connectors/building/authentication.md),
  [review criteria](https://claude.com/docs/connectors/building/review-criteria.md),
  [verification](https://claude.com/docs/connectors/verification.md),
  [directory vs custom](https://claude.com/docs/connectors/building/directory-vs-custom.md),
  [custom connectors](https://www.claude.com/docs/connectors/custom/remote-mcp),
  [MCP Apps](https://claude.com/docs/connectors/building/mcp-apps/getting-started.md)
- OpenAI: [submission](https://developers.openai.com/apps-sdk/deploy/submission),
  [guidelines](https://developers.openai.com/apps-sdk/app-submission-guidelines),
  [auth](https://developers.openai.com/apps-sdk/build/auth),
  [MCP server](https://developers.openai.com/apps-sdk/concepts/mcp-server),
  [developer mode](https://help.openai.com/en/articles/12584461-developer-mode-apps-and-full-mcp-connectors-in-chatgpt) (403, search snippets only)
- MCP: [authorization](https://modelcontextprotocol.io/specification/latest/basic/authorization),
  [Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http.md),
  [changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog.md),
  [subscriptions](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/subscriptions.md),
  [MCP Apps](https://modelcontextprotocol.io/extensions/apps/overview)
- Cloudflare: [Workers pricing](https://developers.cloudflare.com/workers/platform/pricing/),
  [limits](https://developers.cloudflare.com/workers/platform/limits/)
- Secondary: [Apple 5.1.2(i), TechCrunch](https://techcrunch.com/2025/11/13/apples-new-app-review-guidelines-clamp-down-on-apps-sharing-personal-data-with-third-party-ai),
  [Claude subscription auth, The Register](https://www.theregister.com/2026/02/20/anthropic_clarifies_ban_third_party_claude_access)
