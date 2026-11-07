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

/** Take `n` bytes from `owner`'s account: its { used, quota } after, or null if it had no room. */
export async function take(db, owner, n) {
  if (n <= 0) return { used: null, quota: null };
  return db.prepare('UPDATE paper_accounts SET used = used + ?1 WHERE user_id = ?2 AND used + ?1 <= quota AND blocked = 0 RETURNING used, quota')
    .bind(n, owner).first();
}

/** Give `n` bytes back to `owner`'s account. */
export async function give(db, owner, n) {
  if (n <= 0) return;
  await db.prepare('UPDATE paper_accounts SET used = MAX(0, used - ?1) WHERE user_id = ?2').bind(n, owner).run();
}

/** Add `n` bytes to `owner`'s account even past its quota (bringing back what a full copy replaced). */
export async function charge(db, owner, n) {
  if (n <= 0) return;
  await db.prepare('UPDATE paper_accounts SET used = used + ?1 WHERE user_id = ?2').bind(n, owner).run();
}

/** The account row, made with the free quota on first use. */
export async function account(db, user, freeQuota, now = Date.now()) {
  await db.prepare('INSERT OR IGNORE INTO paper_accounts (user_id, quota, used, created_at) VALUES (?1, ?2, 0, ?3)')
    .bind(user, freeQuota, now).run();
  return db.prepare('SELECT user_id, quota, used, blocked FROM paper_accounts WHERE user_id = ?1').bind(user).first();
}

/**
 * A room's lease on its owner's quota. `store` writes it to the room's
 * storage when quota is taken or handed back, and the room writes what is
 * left of it with every frame it keeps (in the same transaction as the
 * frame), so a room that hibernates or is evicted mid-lease hands back only
 * what it really did not spend.
 */
export class Lease {
  constructor(db, chunk, load, store, onTake) {
    this.onTake = onTake; // ({ used, quota }) => void: the account after taking
    this.db = db;
    this.chunk = chunk;
    this.load = load;   // () => { owner, lease }   (as stored)
    this.store = store; // (owner, lease) => void
    this.left = null;   // unspent, in memory
  }

  now() {
    const s = this.load();
    if (this.left == null) this.left = s.lease;
    return { owner: s.owner, lease: this.left };
  }

  /** Spend `n` bytes: false when the owner's account is full. */
  async spend(n) {
    const { owner, lease } = this.now();
    if (!owner) return false;
    if (lease >= n) { this.left = lease - n; return true; }
    const short = n - lease;
    // A chunk ahead if there is room for it, else just what this needs.
    const ahead = Math.max(short, this.chunk);
    let got = ahead > short ? await take(this.db, owner, ahead) : null;
    if (got) { this.left = lease + ahead - n; this.store(owner, this.left); this.onTake?.(got); return true; }
    got = await take(this.db, owner, short);
    if (got) { this.left = 0; this.store(owner, 0); this.onTake?.(got); return true; }
    return false;
  }

  /** Bytes no longer kept: back to the account. */
  async free(n) {
    const { owner } = this.now();
    if (owner) await give(this.db, owner, n);
  }

  /** Hand back what was taken ahead and not spent. */
  async release() {
    const { owner, lease } = this.now();
    if (owner && lease > 0) await give(this.db, owner, lease);
    this.left = 0;
    this.store(owner, 0);
  }
}
