// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// Chat for the people on a page, carried by the page's room on the same
// live socket as the drawing (text frames, see docs/HOSTED.md "Chat").
//
// - Who someone is comes from the socket (the API checked the session and
//   the page's permissions before the room took it), never from a message.
// - Messages are plain text: control and direction-override characters are
//   stripped, the length is capped, and clients render them as text only.
// - Per-account rate limits in the room (across all of an account's sockets).
// - The page's owners turn chat on or off and choose whether it is kept
//   (the last `chatHistory` messages) or ephemeral (gone once everyone has
//   left the page). Settings change through the API (owners only).
// - Deleted with the page (the room's storage goes), exported through the
//   API (GET /pages/:id/chat).

/** The reactions people can add (a fixed set: nothing else is kept). */
export const REACTIONS = ['👍', '❤️', '😂', '🎉', '👀', '✅'];

// C0/C1 controls except \t and \n, zero-width space, BOM, and the bidi
// overrides/isolates that can make text read differently than it is.
const STRIP = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f​‪-‮⁦-⁩﻿]/g;

/** A message as it may be kept: plain text, no tricks, at most `max` characters. */
export function cleanText(s, max = Infinity) {
  const t = String(s ?? '').replace(/\r\n?/g, '\n').replace(STRIP, '').replace(/\n{3,}/g, '\n\n').trim();
  const chars = [...t];
  return { text: chars.slice(0, max).join(''), over: chars.length > max };
}

/**
 * A page's or a person's name: one line, none of the characters above, no
 * `<` or `>` (a second net under every renderer's escaping), at most `max`
 * characters.
 */
export function cleanName(s, max = 120) {
  const t = String(s ?? '').replace(STRIP, '').replace(/[<>]/g, '').replace(/\s+/g, ' ').trim();
  return [...t].slice(0, max).join('');
}

// The word filter's built-in list (slurs; rot13 so the source doesn't
// spell them out). A site sets its own with chatWords / PAPER_CHAT_WORDS.
const BUILT_IN = 'avttre avttref avttn snttbg snttbgf xvxr fcvp puvax genaal jrgonpx pbba tbbx enturnq gbjryurnq';
const rot13 = s => s.replace(/[a-z]/g, c => String.fromCharCode((c.charCodeAt(0) - 97 + 13) % 26 + 97));
export const DEFAULT_WORDS = BUILT_IN.split(' ').map(rot13);

const filters = new Map();
/**
 * Mask `words` in `text` (whole words, any case: "scunthorpe" stays) with
 * asterisks. { text, hits: the words found }.
 */
export function filterWords(text, words = DEFAULT_WORDS) {
  if (!words || !words.length) return { text, hits: [] };
  const key = words.join('\n');
  let re = filters.get(key);
  if (!re) {
    const esc = words.map(w => String(w).toLowerCase().replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).filter(Boolean);
    re = new RegExp(`(?<![\\p{L}\\p{N}_])(?:${esc.join('|')})(?![\\p{L}\\p{N}_])`, 'giu');
    filters.set(key, re);
  }
  const hits = [];
  const out = text.replace(re, m => { hits.push(m.toLowerCase()); return '*'.repeat([...m].length); });
  return { text: out, hits };
}

class Bucket {
  constructor(rate, burst) { this.rate = rate; this.burst = burst; this.level = burst; this.at = Date.now(); }
  take(n = 1) {
    const now = Date.now();
    this.level = Math.min(this.burst, this.level + ((now - this.at) / 1000) * this.rate);
    this.at = now;
    if (this.level < n) return false;
    this.level -= n;
    return true;
  }
}

export class Chat {
  /** room: the PaperRoom (its sql, meta, sockets, send, cfg). */
  constructor(room) {
    this.room = room;
    this.sql = room.sql;
    this.cfg = room.cfg;
    this.setup();
    // Per account: messages, reactions and deletes, typing notes.
    this.buckets = new Map();
    // Accounts already recorded in paper_chatters while this room is awake.
    this.chatted = new Set();
    // When the word filter last reported someone (at most one a minute each).
    this.flagged = new Map();
  }

  /** The room's chat tables (again after the room's storage was deleted). */
  setup() {
    this.sql.exec(`CREATE TABLE IF NOT EXISTS chat (
      id INTEGER PRIMARY KEY AUTOINCREMENT, user TEXT NOT NULL, name TEXT NOT NULL,
      text TEXT NOT NULL, at INTEGER NOT NULL, reacts TEXT NOT NULL DEFAULT '{}')`);
    // Who has blocked whom, for the people who come to this page (from the
    // API on joining, and when someone blocks or unblocks).
    this.sql.exec('CREATE TABLE IF NOT EXISTS chat_blocks (user TEXT NOT NULL, blocked TEXT NOT NULL, PRIMARY KEY (user, blocked))');
  }

  /** Chat at all on this site (PAPER_CHAT). */
  get available() { return !!this.cfg.chat; }

  /** { on, keep }: the owners' choice, else the site's defaults. */
  settings() {
    const on = this.room.meta('chat');
    const keep = this.room.meta('chat_keep');
    return {
      on: on == null ? !!this.cfg.chatDefaultOn : on === '1',
      keep: keep == null ? !!this.cfg.chatDefaultKeep : keep === '1',
      max: this.cfg.chatMaxChars,
      history: this.cfg.chatHistory,
      reactions: REACTIONS,
    };
  }

  /** What a socket is told on joining (null: no chat on this site). */
  welcome() { return this.available ? this.settings() : null; }

  row(r) {
    let reacts = {};
    try { reacts = JSON.parse(r.reacts || '{}'); } catch (e) {}
    return { id: Number(r.id), user: r.user, name: r.name, text: r.text, at: Number(r.at), reacts };
  }
  /** The kept messages, without those of people `forUser` blocked. */
  list(forUser = null) {
    const hide = forUser ? new Set(this.blocksOf(forUser)) : null;
    return this.sql.exec('SELECT * FROM chat ORDER BY id DESC LIMIT ?', this.cfg.chatHistory).toArray().reverse()
      .filter(r => !hide || !hide.has(r.user)).map(r => this.row(r));
  }
  count() { return Number(this.sql.exec('SELECT COUNT(*) AS n FROM chat').toArray()[0].n); }

  // ---- blocks -------------------------------------------------------------------

  blocksOf(user) { return this.sql.exec('SELECT blocked FROM chat_blocks WHERE user = ?', String(user)).toArray().map(r => r.blocked); }
  /** Whether `to` blocked `from`. */
  hides(to, from) { return to !== from && this.sql.exec('SELECT 1 AS x FROM chat_blocks WHERE user = ? AND blocked = ?', String(to), String(from)).toArray().length > 0; }
  /** `user` has blocked exactly `list` (from the API's directory). */
  setBlocks(user, list) {
    user = String(user);
    this.sql.exec('DELETE FROM chat_blocks WHERE user = ?', user);
    for (const b of (list || []).slice(0, this.cfg.maxBlocks)) this.sql.exec('INSERT OR IGNORE INTO chat_blocks (user, blocked) VALUES (?, ?)', user, String(b));
    const now = this.blocksOf(user);
    for (const o of this.room.ctx.getWebSockets('u:' + user)) if (this.room.att(o).mode) this.room.send(o, { t: 'blocks', list: now });
    return now;
  }

  /** After a socket joined: what has been said (if chat is on), without what it blocked. */
  joined(ws) {
    if (!this.available || !this.settings().on) return;
    const a = this.room.att(ws);
    this.room.send(ws, { t: 'blocks', list: this.blocksOf(a.user) });
    this.room.send(ws, { t: 'chat-history', list: this.list(a.user) });
  }

  bucket(user, kind) {
    let b = this.buckets.get(user);
    if (!b) {
      const c = this.cfg;
      b = { msg: new Bucket(c.chatRate, c.chatBurst), act: new Bucket(c.chatRate * 4, c.chatBurst * 2), typing: new Bucket(1, 3) };
      this.buckets.set(user, b);
    }
    return b[kind].take(1);
  }

  /** Everyone on the page (joined sockets), or everyone but `except`; `from`: not to those who blocked them. */
  broadcast(m, except = null, from = null) {
    for (const o of this.room.sockets()) {
      const oa = this.room.att(o);
      if (o === except || !oa.mode) continue;
      if (from != null && this.hides(oa.user, from)) continue;
      this.room.send(o, m);
    }
  }

  /** A chat message from a socket (t: chat, chat-del, chat-react, typing). */
  async handle(ws, a, m) {
    const err = msg => this.room.send(ws, { t: 'chat-error', msg, ref: typeof m.ref === 'string' ? m.ref.slice(0, 40) : undefined });
    if (!this.available) return err('Chat is not available on this site');
    if (!this.settings().on) return err('Chat is off on this page');
    // Visitors without an account (anonymous view links) can read along but not talk.
    if (!a.user || a.user.startsWith('anon:')) return err('Sign in to chat');
    if (m.t === 'typing') {
      if (!this.bucket(a.user, 'typing')) return;
      return this.broadcast({ t: 'typing', user: a.user, name: a.name, on: !!m.on }, ws, a.user);
    }
    if (m.t === 'chat') {
      const clean = cleanText(m.text, this.cfg.chatMaxChars);
      if (clean.over) return err(`That message is too long (at most ${this.cfg.chatMaxChars} characters)`);
      if (!clean.text) return;
      if (!this.bucket(a.user, 'msg')) return err('Slow down: too many messages at once');
      // The word filter: masked for everyone, and the admin is told.
      const { text, hits } = filterWords(clean.text, this.cfg.chatWords ?? DEFAULT_WORDS);
      const at = Date.now();
      this.sql.exec('INSERT INTO chat (user, name, text, at) VALUES (?, ?, ?, ?)', a.user, a.name || '', text, at);
      const id = Number(this.sql.exec('SELECT last_insert_rowid() AS s').toArray()[0].s);
      // Only the last `chatHistory` messages stay.
      this.sql.exec('DELETE FROM chat WHERE id <= ?', id - this.cfg.chatHistory);
      const msg = { id, user: a.user, name: a.name || '', text, at, reacts: {} };
      for (const o of this.room.sockets()) {
        const oa = this.room.att(o);
        if (!oa.mode || (o !== ws && this.hides(oa.user, a.user))) continue;
        this.room.send(o, o === ws && typeof m.ref === 'string' ? { t: 'chat', msg, ref: m.ref.slice(0, 40) } : { t: 'chat', msg });
      }
      await this.room.chatted(a.user, this.chatted);
      if (hits.length) await this.flag(a, msg, clean.text, hits);
      return;
    }
    if (m.t === 'chat-del' || m.t === 'chat-react') {
      if (!this.bucket(a.user, 'act')) return err('Slow down a little');
      const id = Number(m.id);
      const r = Number.isInteger(id) ? this.sql.exec('SELECT * FROM chat WHERE id = ?', id).toArray()[0] : null;
      if (!r) return;
      if (m.t === 'chat-del') {
        // Your own messages; the page's owners may take down anyone's.
        if (r.user !== a.user && a.role !== 'owner') return err('You can only delete your own messages');
        this.sql.exec('DELETE FROM chat WHERE id = ?', id);
        return this.broadcast({ t: 'chat-del', id });
      }
      if (!REACTIONS.includes(m.emoji)) return;
      const reacts = this.row(r).reacts;
      const who = reacts[m.emoji] || [];
      const i = who.findIndex(x => x.user === a.user);
      if (i >= 0) who.splice(i, 1); else who.push({ user: a.user, name: a.name || '' });
      if (who.length) reacts[m.emoji] = who.slice(0, 64); else delete reacts[m.emoji];
      this.sql.exec('UPDATE chat SET reacts = ? WHERE id = ?', JSON.stringify(reacts), id);
      this.broadcast({ t: 'chat-react', id, reacts });
      // Reactions carry the account too: found again if it is deleted.
      await this.room.chatted(a.user, this.chatted);
    }
  }

  /** The word filter caught something: a report for the admin (one a minute per person). */
  async flag(a, msg, original, hits) {
    const last = this.flagged.get(a.user) || 0;
    if (Date.now() - last < 60_000) return;
    this.flagged.set(a.user, Date.now());
    await this.room.report({
      kind: 'chat-auto', reporter: null, subject: a.user,
      reason: `Chat word filter: ${[...new Set(hits)].join(', ')}`.slice(0, 300),
      context: { message: { id: msg.id, user: a.user, name: msg.name, text: original, at: msg.at }, before: this.around(msg.id, 3).before },
    });
  }

  /** Messages around `id` (for a report): { message, before, after }; message null when it is gone. */
  around(id, n = 3) {
    id = Number(id);
    const cut = x => ({ id: x.id, user: x.user, name: x.name, text: x.text, at: x.at });
    const r = Number.isInteger(id) ? this.sql.exec('SELECT * FROM chat WHERE id = ?', id).toArray()[0] : null;
    if (!r) return { message: null, before: [], after: [] };
    const before = this.sql.exec('SELECT * FROM chat WHERE id < ? ORDER BY id DESC LIMIT ?', id, n).toArray().reverse().map(x => cut(this.row(x)));
    const after = this.sql.exec('SELECT * FROM chat WHERE id > ? ORDER BY id LIMIT ?', id, n).toArray().map(x => cut(this.row(x)));
    return { message: cut(this.row(r)), before, after };
  }

  // ---- from the API (already checked: owners only, or members for export) ----

  /** Owners: turn chat on/off, kept or ephemeral. */
  set({ on, keep }) {
    if (typeof on === 'boolean') this.room.setMeta('chat', on ? '1' : '0');
    if (typeof keep === 'boolean') this.room.setMeta('chat_keep', keep ? '1' : '0');
    const s = this.settings();
    this.broadcast({ t: 'chat-settings', ...s });
    // Turned on while people are here: show them what is kept.
    if (s.on && on === true) for (const o of this.room.sockets()) if (this.room.att(o).mode) this.joined(o);
    return s;
  }

  /** Owners: clear the chat for everyone. */
  clear() {
    const n = this.count();
    this.sql.exec('DELETE FROM chat');
    this.broadcast({ t: 'chat-clear' });
    return { cleared: n };
  }

  /** The admin: take one message down. */
  remove(id) {
    id = Number(id);
    const had = Number.isInteger(id) && this.sql.exec('SELECT 1 AS x FROM chat WHERE id = ?', id).toArray().length > 0;
    if (!had) return { removed: 0 };
    this.sql.exec('DELETE FROM chat WHERE id = ?', id);
    this.broadcast({ t: 'chat-del', id });
    return { removed: 1 };
  }

  /** An account was deleted: its messages, its reactions and its blocks go. */
  forget(user) {
    user = String(user);
    const mine = this.sql.exec('SELECT id FROM chat WHERE user = ?', user).toArray().map(r => Number(r.id));
    this.sql.exec('DELETE FROM chat WHERE user = ?', user);
    for (const id of mine) this.broadcast({ t: 'chat-del', id });
    let reacts = 0;
    for (const r of this.sql.exec('SELECT id, reacts FROM chat').toArray()) {
      const x = this.row(r).reacts;
      let changed = false;
      for (const [e, who] of Object.entries(x)) {
        const keep = who.filter(w => w.user !== user);
        if (keep.length === who.length) continue;
        changed = true;
        if (keep.length) x[e] = keep; else delete x[e];
      }
      if (!changed) continue;
      reacts++;
      this.sql.exec('UPDATE chat SET reacts = ? WHERE id = ?', JSON.stringify(x), r.id);
      this.broadcast({ t: 'chat-react', id: Number(r.id), reacts: x });
    }
    this.sql.exec('DELETE FROM chat_blocks WHERE user = ? OR blocked = ?', user, user);
    this.buckets.delete(user);
    this.chatted.delete(user);
    return { messages: mine.length, reactions: reacts };
  }

  /** Data export: the settings and every kept message (not those of people `forUser` blocked). */
  export(forUser = null) { return { settings: this.settings(), messages: this.list(forUser) }; }

  /** Everyone left: an ephemeral chat goes with them. */
  empty() {
    this.buckets.clear();
    if (!this.settings().keep) this.sql.exec('DELETE FROM chat');
  }
}
