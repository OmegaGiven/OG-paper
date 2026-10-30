// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// OG Paper, hosted: accounts' pages, sharing, quotas and live rooms for a
// Cloudflare Worker site. See docs/HOSTED.md.
//
//   import { createPaperApi, PaperRoom } from 'og-paper-hosted';
//   const paper = createPaperApi({ auth: myAdapter });
//   export { PaperRoom };                       // the Durable Object class
//   export default { async fetch(req, env, ctx) {
//     return (await paper(req, env, ctx)) ?? fallback(req, env);
//   } };
//
// The auth adapter is how the host site says who is signed in (there is no
// user system here):
//
//   {
//     user(req, env)        -> { id, name } | null  the session's account
//     find(env, who)        -> { id, name } | null  by username or email (sharing)
//     names(env, ids)       -> { [id]: name }       display names (optional)
//     isAdmin(user, env)    -> boolean              may moderate (optional)
//     limit(env, kind, key) -> boolean              true: over a rate limit (optional;
//                                                   kind: api, write, create, ws)
//   }

export { createApi as createPaperApi } from './api.js';
export { PaperRoom } from './room.js';
export { DEFAULTS as PAPER_DEFAULTS } from './config.js';
