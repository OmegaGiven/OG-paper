// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// A reference client for hosted rooms, in plain JavaScript: the merge model
// of ogpaper-core's `sync` (items never edited in place, show/hide events
// stamped with a hybrid logical clock, rival edits settled by the newest
// edit) and the hosted protocol's extras (resume from a seq, tiles). The
// Rust app is the real client; this one documents the protocol in code,
// drives the tests, and is a start for scripts and tile-aware apps.
//
// Changes travel as JSON here; the Rust app sends sealed `.ogpt` changes.
// The room treats both the same: it never reads them.

import { READY, TAG, enc, dec, putTiled, readSeq, tagOf, withTag } from '../src/protocol.js';

/** Hybrid logical clock stamps as [ms, n, peer]; compared in that order. */
export const cmp = (a, b) => (a[0] - b[0]) || (a[1] - b[1]) || (a[2] < b[2] ? -1 : a[2] > b[2] ? 1 : 0);

export class Log {
  constructor(peer, now = () => Date.now()) {
    this.peer = peer;
    this.now = now;
    this.last = [0, 0, peer];
    this.items = new Map();    // id -> { tile, data }
    this.events = new Map();   // `${id}|${at}` -> { item, alive, at }
    this.replaces = new Map(); // item -> { item, replaces, edit }
  }

  stamp() {
    const ms = this.now();
    this.last = ms > this.last[0] ? [ms, 0, this.peer] : [this.last[0], this.last[1] + 1, this.peer];
    return this.last;
  }
  observe(h) { if (cmp([h[0], h[1], ''], [this.last[0], this.last[1], '']) > 0) this.last = [h[0], h[1], this.peer]; }

  // ---- edits (what an app does) ----
  add(id, tile, data) {
    this.items.set(id, { tile, data });
    this.event(id, true, this.stamp());
    return id;
  }
  remove(id) { this.event(id, false, this.stamp()); }
  /** Edit an item: hide it, add its replacement (one edit, one stamp). */
  replace(old, id, tile, data) {
    const at = this.stamp();
    this.items.set(id, { tile, data });
    this.event(old, false, at);
    this.event(id, true, at);
    this.replaces.set(id, { item: id, replaces: old, edit: at });
  }
  event(item, alive, at) { this.events.set(`${item}|${at.join('.')}`, { item, alive, at }); }

  // ---- state ----
  /** Ids shown now: each item's latest event, then rival edits settled. */
  visible() {
    const latest = new Map();
    for (const e of this.events.values()) {
      const l = latest.get(e.item);
      if (!l || cmp(e.at, l.at) > 0) latest.set(e.item, e);
    }
    const shown = new Set([...latest.values()].filter(e => e.alive).map(e => e.item));
    // Rival branches: several items replacing the same one; the newest edit wins.
    const rivals = new Map();
    for (const r of this.replaces.values()) {
      if (!rivals.has(r.replaces)) rivals.set(r.replaces, []);
      rivals.get(r.replaces).push(r);
    }
    for (const list of rivals.values()) {
      if (list.length < 2) continue;
      list.sort((a, b) => cmp(b.edit, a.edit));
      for (const lost of list.slice(1)) this.hideBranch(lost.item, shown);
    }
    return shown;
  }
  hideBranch(item, shown) {
    shown.delete(item);
    for (const r of this.replaces.values()) if (r.replaces === item) this.hideBranch(r.item, shown);
  }
  /** What this copy has: the latest stamp per peer. */
  version() {
    const v = {};
    for (const e of this.events.values()) if (!v[e.at[2]] || cmp(e.at, v[e.at[2]]) > 0) v[e.at[2]] = e.at;
    return v;
  }
  /** The changes a copy that has `v` lacks (and the tiles they touch). */
  since(v = {}) {
    const has = at => v[at[2]] && cmp(at, v[at[2]]) <= 0;
    const events = [...this.events.values()].filter(e => !has(e.at));
    const ids = new Set(events.map(e => e.item));
    const items = [...ids].filter(id => this.items.has(id)).map(id => [id, this.items.get(id)]);
    const replaces = [...this.replaces.values()].filter(r => ids.has(r.item));
    const tiles = [...new Set(items.map(([, it]) => it.tile).filter(Boolean))];
    return { events, items, replaces, tiles };
  }
  merge(c) {
    for (const [id, it] of c.items) if (!this.items.has(id)) this.items.set(id, it);
    for (const e of c.events) { this.events.set(`${e.item}|${e.at.join('.')}`, e); this.observe(e.at); }
    for (const r of c.replaces) this.replaces.set(r.item, r);
  }
}

export const joinVersion = (a, b) => {
  const out = { ...a };
  for (const [p, h] of Object.entries(b)) if (!out[p] || cmp(h, out[p]) > 0) out[p] = h;
  return out;
};

/**
 * A client of one hosted page. `dial()` opens a WebSocket-like connection
 * (send, close, and onmessage/onclose set by us). It keeps working offline
 * and sends what the room lacks when it reconnects, asking only for what
 * came after the last seq it saw. `tiles`: only those tiles (null: all).
 */
export class HostedClient {
  constructor({ peer, dial, tiles = null, room = null, now }) {
    this.log = new Log(peer, now);
    this.dial = dial;
    this.tiles = tiles;
    this.room = room;
    this.since = 0;          // the last kept frame seen, in the room's order
    this.their = {};         // what the room has (as far as we know)
    this.ws = null;
    this.live = false;
    this.notes = [];         // the room's control messages, newest last
    this.peers = [];
    this.received = 0;       // kept frames received (for tests)
    this.inflight = [];
  }

  async connect() {
    const ws = await this.dial();
    if (!ws || typeof ws.send !== 'function') { this.refused = ws; return false; }
    this.refused = null;
    this.ws = ws;
    this.live = false;
    ws.onmessage = m => this.onMessage(m);
    ws.onclose = () => { if (this.ws === ws) { this.ws = null; this.live = false; } };
    await ws.send(JSON.stringify({ t: 'hello', since: this.since, seq: true, tiles: this.tiles, room: this.room }));
    // What the room lacks goes up as soon as it is ready.
    await this.pending;
    return true;
  }
  disconnect() { const ws = this.ws; this.ws = null; this.live = false; ws?.close(); }

  onMessage(data) {
    if (typeof data === 'string') {
      const m = JSON.parse(data);
      this.notes.push(m);
      if (m.t === 'ready') { this.live = true; this.pending = this.push(); return; }
      // Each kept frame sent gets one answer: kept (ack), or not (full, denied).
      if (m.t === 'ack') { this.since = Math.max(this.since, m.seq); const v = this.inflight?.shift(); if (v) this.their = joinVersion(this.their, v); return; }
      if (m.t === 'full' || m.t === 'denied') { this.inflight?.shift(); return; }
      if (m.t === 'peers') this.peers = m.list;
      return;
    }
    const b = new Uint8Array(data);
    if (b.length === READY.length && dec.decode(b) === 'READY') { this.live = true; this.pending = this.push(); return; }
    const s = readSeq(b);
    if (!s) return;
    this.received++;
    this.since = Math.max(this.since, s.seq);
    const c = JSON.parse(dec.decode(s.body));
    if (c.kind === 'changes' || c.kind === 'ckpt') {
      this.log.merge(c);
      this.their = joinVersion(this.their, versionOf(c));
    }
  }

  /** Send the room what it lacks. */
  push() {
    if (!this.live || !this.ws) return;
    const c = this.log.since(this.their);
    if (c.events.length === 0) return;
    const body = enc.encode(JSON.stringify({ kind: 'changes', events: c.events, items: c.items, replaces: c.replaces }));
    (this.inflight ||= []).push(versionOf(c));
    const frame = c.tiles.length ? putTiled(c.tiles, body) : withTag(TAG.PUT, body);
    return this.ws.send(frame);
  }

  /** A full copy for the room to keep instead of what it covers. */
  checkpoint() {
    const c = this.log.since({});
    const body = enc.encode(JSON.stringify({ kind: 'ckpt', events: c.events, items: c.items, replaces: c.replaces }));
    (this.inflight ||= []).push(versionOf(c));
    return this.ws.send(withTag(TAG.CKPT, body));
  }

  subscribe(add, remove = []) {
    this.tiles = [...(this.tiles || []).filter(t => !remove.includes(t)), ...add];
    return this.ws.send(JSON.stringify({ t: 'subs', add, remove }));
  }

  /** A view or a stroke in progress (passed on, never kept). */
  ephemeral(obj) { return this.ws?.send(withTag(TAG.EPH, enc.encode(JSON.stringify({ kind: 'eph', ...obj })))); }

  /** A note to the room itself (chat, typing: see HOSTED.md "Chat"). */
  control(obj) { return this.ws?.send(JSON.stringify(obj)); }
  say(text, ref) { return this.control({ t: 'chat', text, ref }); }

  shown() { return this.log.visible(); }
}

function versionOf(c) {
  const v = {};
  for (const e of c.events) if (!v[e.at[2]] || cmp(e.at, v[e.at[2]]) > 0) v[e.at[2]] = e.at;
  return v;
}

export { tagOf };
