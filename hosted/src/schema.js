// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// What the D1 directory gained after migrations/0001_og_paper.sql. The API
// adds it itself the first time it runs (once per Worker isolate: one read
// when it is already there), so a site that updates og-paper needs no new
// D1 migration of its own:
//
//   paper_pages.deleted_at   in the trash since (ms); purged after trashDays
//   paper_transfers          give-away offers waiting for the recipient
//   paper_reports.kind, subject_id, context
//                            what a report is about: 'page' (null), 'chat' (a
//                            message someone reported) or 'chat-auto' (the
//                            word filter); whose message; a snapshot (JSON)
//   paper_chatters           pages each account has chatted on (to find its
//                            messages when the account is deleted)
//   paper_blocks             people an account has blocked
//
// The new tables are also in migrations/0002_og_paper_safety.sql (CREATE
// TABLE IF NOT EXISTS: safe either way round); the columns only here.
//
// Don't turn these into a `wrangler d1 migrations` file: applied after the
// API has run, the ALTER would fail (the column is already there).
//
// paper_transfers: "X wants to give you this page". One offer per page;
// nothing moves until the recipient accepts; offers lapse after offerDays.

export const UPGRADE = [
  'ALTER TABLE paper_pages ADD COLUMN deleted_at INTEGER',
  `CREATE TABLE IF NOT EXISTS paper_transfers (
    page_id     TEXT PRIMARY KEY,
    from_id     TEXT NOT NULL,
    to_id       TEXT NOT NULL,
    leave       INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
  )`,
  'CREATE INDEX IF NOT EXISTS paper_transfers_to ON paper_transfers(to_id, created_at)',
  'CREATE INDEX IF NOT EXISTS paper_pages_trash ON paper_pages(deleted_at)',
  'ALTER TABLE paper_reports ADD COLUMN kind TEXT',
  'ALTER TABLE paper_reports ADD COLUMN subject_id TEXT',
  'ALTER TABLE paper_reports ADD COLUMN context TEXT',
  `CREATE TABLE IF NOT EXISTS paper_chatters (
    page_id     TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    PRIMARY KEY (page_id, user_id)
  )`,
  'CREATE INDEX IF NOT EXISTS paper_chatters_user ON paper_chatters(user_id)',
  `CREATE TABLE IF NOT EXISTS paper_blocks (
    user_id     TEXT NOT NULL,
    blocked_id  TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    PRIMARY KEY (user_id, blocked_id)
  )`,
  'CREATE INDEX IF NOT EXISTS paper_blocks_blocked ON paper_blocks(blocked_id)',
];

const done = new WeakMap();

/** Make sure the directory has what this version of the API uses. */
export function ensureSchema(db) {
  if (!db) return Promise.resolve();
  let p = done.get(db);
  if (!p) {
    p = upgrade(db).catch(e => { done.delete(db); throw e; });
    done.set(db, p);
  }
  return p;
}

async function upgrade(db) {
  try {
    await db.prepare('SELECT p.deleted_at, r.context, c.user_id, b.blocked_id FROM paper_transfers t, paper_pages p, paper_reports r, paper_chatters c, paper_blocks b LIMIT 0').all();
    return;
  } catch (e) { /* not yet */ }
  for (const sql of UPGRADE) {
    try { await db.prepare(sql).run(); } catch (e) {
      // Two isolates upgrading at once: the column is already there.
      if (!/duplicate column/i.test(String(e?.message || e))) throw e;
    }
  }
}
