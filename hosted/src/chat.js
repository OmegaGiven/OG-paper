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
    this.sql.exec(`CREATE TABLE IF NOT EXISTS chat (
      id INTEGER PRIMARY KEY AUTOINCREMENT, user TEXT NOT NULL, name TEXT NOT NULL,
      text TEXT NOT NULL, at INTEGER NOT NULL, reacts TEXT NOT NULL DEFAULT '{}')`);
    // Per account: messages, reactions and deletes, typing notes.
    this.buckets = new Map();
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
  list() {
    return this.sql.exec('SELECT * FROM chat ORDER BY id DESC LIMIT ?', this.cfg.chatHistory).toArray().reverse().map(r => this.row(r));
  }
  count() { return Number(this.sql.exec('SELECT COUNT(*) AS n FROM chat').toArray()[0].n); }

  /** After a socket joined: what has been said (if chat is on). */
  joined(ws) {
    if (!this.available || !this.settings().on) return;
    this.room.send(ws, { t: 'chat-history', list: this.list() });
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

  /** Everyone on the page (joined sockets), or everyone but `except`. */
  broadcast(m, except = null) {
    for (const o of this.room.sockets()) if (o !== except && this.room.att(o).mode) this.room.send(o, m);
  }

  /** A chat message from a socket (t: chat, chat-del, chat-react, typing). */
  handle(ws, a, m) {
    const err = msg => this.room.send(ws, { t: 'chat-error', msg, ref: typeof m.ref === 'string' ? m.ref.slice(0, 40) : undefined });
    if (!this.available) return err('Chat is not available on this site');
    if (!this.settings().on) return err('Chat is off on this page');
    // Visitors without an account (anonymous view links) can read along but not talk.
    if (!a.user || a.user.startsWith('anon:')) return err('Sign in to chat');
    if (m.t === 'typing') {
      if (!this.bucket(a.user, 'typing')) return;
      return this.broadcast({ t: 'typing', user: a.user, name: a.name, on: !!m.on }, ws);
    }
    if (m.t === 'chat') {
      const { text, over } = cleanText(m.text, this.cfg.chatMaxChars);
      if (over) return err(`That message is too long (at most ${this.cfg.chatMaxChars} characters)`);
      if (!text) return;
      if (!this.bucket(a.user, 'msg')) return err('Slow down: too many messages at once');
      const at = Date.now();
      this.sql.exec('INSERT INTO chat (user, name, text, at) VALUES (?, ?, ?, ?)', a.user, a.name || '', text, at);
      const id = Number(this.sql.exec('SELECT last_insert_rowid() AS s').toArray()[0].s);
      // Only the last `chatHistory` messages stay.
      this.sql.exec('DELETE FROM chat WHERE id <= ?', id - this.cfg.chatHistory);
      const msg = { id, user: a.user, name: a.name || '', text, at, reacts: {} };
      for (const o of this.room.sockets()) {
        if (!this.room.att(o).mode) continue;
        this.room.send(o, o === ws && typeof m.ref === 'string' ? { t: 'chat', msg, ref: m.ref.slice(0, 40) } : { t: 'chat', msg });
      }
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
      return this.broadcast({ t: 'chat-react', id, reacts });
    }
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

  /** Data export: the settings and every kept message. */
  export() { return { settings: this.settings(), messages: this.list() }; }

  /** Everyone left: an ephemeral chat goes with them. */
  empty() {
    this.buckets.clear();
    if (!this.settings().keep) this.sql.exec('DELETE FROM chat');
  }
}
