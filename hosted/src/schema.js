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
//   paper_app_grants/_codes  native apps signed in to an account (oauth.js)
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
  // Signing in from the native apps (see oauth.js): one grant per device.
  `CREATE TABLE IF NOT EXISTS paper_app_grants (
    id                 TEXT PRIMARY KEY,
    user_id            TEXT NOT NULL,
    device             TEXT NOT NULL,
    access_hash        TEXT NOT NULL UNIQUE,
    access_expires     INTEGER NOT NULL,
    refresh_hash       TEXT NOT NULL UNIQUE,
    refresh_expires    INTEGER NOT NULL,
    prev_refresh_hash  TEXT,
    rotated_at         INTEGER,
    created_at         INTEGER NOT NULL,
    last_used          INTEGER NOT NULL
  )`,
  'CREATE INDEX IF NOT EXISTS paper_app_grants_user ON paper_app_grants(user_id)',
  'CREATE INDEX IF NOT EXISTS paper_app_grants_prev ON paper_app_grants(prev_refresh_hash)',
  `CREATE TABLE IF NOT EXISTS paper_app_codes (
    code_hash     TEXT PRIMARY KEY,
    user_id       TEXT NOT NULL,
    redirect_uri  TEXT NOT NULL,
    challenge     TEXT NOT NULL,
    device        TEXT NOT NULL,
    expires_at    INTEGER NOT NULL
  )`,
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
    await db.prepare('SELECT deleted_at, rotated_at, expires_at FROM paper_transfers, paper_pages, paper_app_grants, paper_app_codes LIMIT 0').all();
    return;
  } catch (e) { /* not yet */ }
  for (const sql of UPGRADE) {
    try { await db.prepare(sql).run(); } catch (e) {
      // Two isolates upgrading at once: the column is already there.
      if (!/duplicate column/i.test(String(e?.message || e))) throw e;
    }
  }
}
