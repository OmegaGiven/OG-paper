// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// The auth adapter for omegagiven.com (its accounts: users, identities and
// sessions in D1, src/auth.js; rate limits in src/guard.js). The site passes
// its own functions in, so this file imports nothing from it:
//
//   import { currentUser } from './auth.js';
//   import { limited } from './guard.js';
//   import { omegagivenAuth } from '../vendor/og-paper-hosted/adapters/omegagiven.js';
//   const paper = createPaperApi({ auth: omegagivenAuth({ currentUser, limited }) });
//
// Admins: PAPER_ADMINS, a comma-separated list of user ids (a var or secret).

const LIMITS = { api: 'RL_API', write: 'RL_WRITE', create: 'RL_CREATE', ws: 'RL_WS' };

export function omegagivenAuth({ currentUser, limited }) {
  return {
    async user(req, env) {
      const u = await currentUser(req, env);
      return u ? { id: String(u.id), name: u.username || null } : null;
    },
    // Share by username (exact, any case) or by an email a sign-in reported.
    async find(env, who) {
      who = String(who).trim().replace(/^@/, '');
      if (!who) return null;
      const row = who.includes('@')
        ? await env.DB.prepare('SELECT u.id, u.username FROM identities i JOIN users u ON u.id = i.user_id WHERE i.email = ?1 COLLATE NOCASE LIMIT 1').bind(who).first()
        : await env.DB.prepare('SELECT id, username FROM users WHERE username = ?1').bind(who).first();
      return row ? { id: String(row.id), name: row.username || null } : null;
    },
    async names(env, ids) {
      const nums = ids.map(Number).filter(Number.isInteger).slice(0, 100);
      if (!nums.length) return {};
      const q = `SELECT id, username FROM users WHERE id IN (${nums.map((_, i) => '?' + (i + 1)).join(',')})`;
      const rows = (await env.DB.prepare(q).bind(...nums).all()).results;
      return Object.fromEntries(rows.map(r => [String(r.id), r.username || null]));
    },
    isAdmin(user, env) {
      return String(env.PAPER_ADMINS || '').split(',').map(s => s.trim()).filter(Boolean).includes(String(user.id));
    },
    limit(env, kind, key) {
      return limited ? limited(env, LIMITS[kind] || 'RL_API', 'paper:' + key) : false;
    },
  };
}
