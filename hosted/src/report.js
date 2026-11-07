// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// Reports for the admin (paper_reports) and the site's onReport hook. The
// API files reports people make; rooms file the word filter's. Both run in
// the same Worker, so the hook createPaperApi was given is kept here for
// the rooms too.

let hook = null;

/** The site's onReport (from createPaperApi). */
export function setReportHook(fn) { if (typeof fn === 'function') hook = fn; }

/**
 * Keep a report and tell the site. r: { page, reporter, reason, kind?,
 * subject?, context? } (context: anything JSON). waitUntil keeps the hook
 * running after the response. Returns the report's id.
 */
export async function fileReport(db, r, env, waitUntil) {
  const row = await db.prepare('INSERT INTO paper_reports (page_id, reporter_id, reason, created_at, kind, subject_id, context) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id')
    .bind(r.page, r.reporter ?? null, r.reason, Date.now(), r.kind ?? null, r.subject ?? null, r.context == null ? null : JSON.stringify(r.context)).first();
  const out = { id: row.id, page: r.page, reporter: r.reporter ?? null, reason: r.reason };
  if (r.kind) Object.assign(out, { kind: r.kind, subject: r.subject ?? null, context: r.context ?? null });
  let p;
  try { p = hook && hook(out, env); } catch (e) { console.error('og-paper onReport', e); }
  if (p && typeof p.then === 'function') {
    const done = Promise.resolve(p).catch(e => console.error('og-paper onReport', e));
    if (waitUntil) waitUntil(done);
  }
  return row.id;
}
