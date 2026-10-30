// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// Quota accounting. An account's `used` (D1, paper_accounts) is everything
// its pages keep plus what busy rooms have taken ahead ("leases"):
//
//   used = Σ page bytes + Σ unspent leases
//
// A room takes quota from its page's billing owner a chunk at a time with
// one conditional UPDATE (`used + n <= quota`), so the account can never go
// over, however many of its pages are being drawn on at once, and a busy
// room writes to D1 once per chunk rather than once per stroke. It hands
// back what it did not spend when it goes quiet, and frees what a
// checkpoint or a deletion no longer keeps.

/** Take `n` bytes from `owner`'s account: true if it had room. */
export async function take(db, owner, n) {
  if (n <= 0) return true;
  const r = await db.prepare('UPDATE paper_accounts SET used = used + ?1 WHERE user_id = ?2 AND used + ?1 <= quota AND blocked = 0')
    .bind(n, owner).run();
  return (r.meta?.changes ?? 0) > 0;
}

/** Give `n` bytes back to `owner`'s account. */
export async function give(db, owner, n) {
  if (n <= 0) return;
  await db.prepare('UPDATE paper_accounts SET used = MAX(0, used - ?1) WHERE user_id = ?2').bind(n, owner).run();
}

/** The account row, made with the free quota on first use. */
export async function account(db, user, freeQuota, now = Date.now()) {
  await db.prepare('INSERT OR IGNORE INTO paper_accounts (user_id, quota, used, created_at) VALUES (?1, ?2, 0, ?3)')
    .bind(user, freeQuota, now).run();
  return db.prepare('SELECT user_id, quota, used, blocked FROM paper_accounts WHERE user_id = ?1').bind(user).first();
}

/**
 * A room's lease on its owner's quota (the lease itself is kept in the
 * room's storage, so a room that is evicted gives it back when it wakes).
 */
export class Lease {
  constructor(db, chunk, load, store) {
    this.db = db;
    this.chunk = chunk;
    this.load = load;   // () => { owner, lease }
    this.store = store; // (owner, lease) => void
  }

  /** Spend `n` bytes: false when the owner's account is full. */
  async spend(n) {
    const { owner, lease } = this.load();
    if (!owner) return false;
    if (lease >= n) { this.store(owner, lease - n); return true; }
    const short = n - lease;
    // A chunk ahead if there is room for it, else just what this needs.
    const ahead = Math.max(short, this.chunk);
    if (ahead > short && await take(this.db, owner, ahead)) { this.store(owner, lease + ahead - n); return true; }
    if (await take(this.db, owner, short)) { this.store(owner, 0); return true; }
    return false;
  }

  /** Bytes no longer kept: back to the account. */
  async free(n) {
    const { owner } = this.load();
    if (owner) await give(this.db, owner, n);
  }

  /** Hand back what was taken ahead and not spent. */
  async release() {
    const { owner, lease } = this.load();
    if (owner && lease > 0) await give(this.db, owner, lease);
    this.store(owner, 0);
  }
}
