// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// The live protocol between an app and a page's room (see docs/HOSTED.md).
//
// Binary frames are the relay protocol every OG Paper app already speaks
// (crates/og-paper/src/relay.rs): `ROOM` + 32-byte room first, then `PUT `
// (kept and passed on), `EPH ` (only passed on), `CKPT` (a full copy, kept
// instead of what it covers). The room cannot read any of them: they are
// sealed end to end. Hosted rooms add, for apps that ask for them:
//
// - `PUTT` + tiles + sealed frame: a change that touches these tiles.
// - `TCKP` + one tile + sealed frame: a full copy of one tile.
//   Tiles are short opaque strings the app picks (an ancestor cell of the
//   strokes, see HOSTED.md); a list is u8 count + (u8 length + UTF-8) each.
// - `SEQ ` + u64 LE seq + sealed frame (room to app): a kept frame and its
//   place in the log, so a reconnect asks only for what came after.
//
// Text frames are the room's own control channel, JSON: app to room
// `{t:'hello', since, seq, tiles}` (instead of ROOM) and `{t:'subs', add,
// remove}`; room to app `{t:'welcome'}`, `{t:'head'}`, `{t:'peers'}`,
// `{t:'full'}`, `{t:'denied'}`, `{t:'error'}`, `{t:'kicked'}`. Apps that do
// not know them (the Rust app) never see them: the page glue reads text
// frames and passes only binary ones on.

export const enc = new TextEncoder();
export const dec = new TextDecoder();

export const TAG = {
  ROOM: 'ROOM',
  PUT: 'PUT ',
  EPH: 'EPH ',
  CKPT: 'CKPT',
  PUTT: 'PUTT',
  TCKP: 'TCKP',
  SEQ: 'SEQ ',
};
export const READY = enc.encode('READY');

/** Kinds of kept frame. */
export const KIND = { PUT: 1, CKPT: 2, TCKP: 3 };

export const tagOf = b => (b.length >= 4 ? String.fromCharCode(b[0], b[1], b[2], b[3]) : '');

export function withTag(tag, body) {
  const out = new Uint8Array(4 + body.length);
  out.set(enc.encode(tag), 0);
  out.set(body, 4);
  return out;
}

/** Encode a tile list (at most 255 tiles of at most 255 bytes each). */
export function encodeTiles(tiles) {
  const parts = tiles.map(t => enc.encode(t));
  const out = new Uint8Array(1 + parts.reduce((n, p) => n + 1 + p.length, 0));
  out[0] = parts.length;
  let at = 1;
  for (const p of parts) { out[at++] = p.length; out.set(p, at); at += p.length; }
  return out;
}

/** Read a tile list at `at`: { tiles, at } (where the rest starts), or null. */
export function decodeTiles(b, at = 0) {
  if (at >= b.length) return null;
  const n = b[at++];
  const tiles = [];
  for (let i = 0; i < n; i++) {
    if (at >= b.length) return null;
    const len = b[at++];
    if (at + len > b.length) return null;
    tiles.push(dec.decode(b.subarray(at, at + len)));
    at += len;
  }
  return { tiles, at };
}

/** `PUTT` from an app: tiles then the sealed frame. */
export function putTiled(tiles, sealed) {
  const t = encodeTiles(tiles);
  const body = new Uint8Array(t.length + sealed.length);
  body.set(t, 0);
  body.set(sealed, t.length);
  return withTag(TAG.PUTT, body);
}

/** `SEQ ` to an app: a kept frame and its seq. */
export function seqFrame(seq, sealed) {
  const out = new Uint8Array(12 + sealed.length);
  out.set(enc.encode(TAG.SEQ), 0);
  new DataView(out.buffer).setBigUint64(4, BigInt(seq), true);
  out.set(sealed, 12);
  return out;
}

/** Read a `SEQ ` frame: { seq, body }, or null. */
export function readSeq(b) {
  if (tagOf(b) !== TAG.SEQ || b.length < 12) return null;
  const seq = Number(new DataView(b.buffer, b.byteOffset, b.byteLength).getBigUint64(4, true));
  return { seq, body: b.subarray(12) };
}

/**
 * Read a frame from an app: { tag, tiles, body } where body is what is
 * kept or passed on (the sealed frame), or null when malformed.
 */
export function parseFrame(b) {
  const tag = tagOf(b);
  switch (tag) {
    case TAG.ROOM:
      return b.length === 36 ? { tag, room: b.subarray(4), body: null, tiles: null } : null;
    case TAG.PUT:
    case TAG.EPH:
    case TAG.CKPT:
      return { tag, tiles: null, body: b.subarray(4) };
    case TAG.PUTT:
    case TAG.TCKP: {
      const t = decodeTiles(b, 4);
      if (!t) return null;
      if (tag === TAG.TCKP && t.tiles.length !== 1) return null;
      return { tag, tiles: t.tiles, body: b.subarray(t.at) };
    }
    default:
      return null;
  }
}

/** Whether a kept frame with these tiles belongs to a socket with these subscriptions. */
export function wants(subs, tiles) {
  if (!subs || !tiles || tiles.length === 0) return true;
  for (const t of tiles) if (subs.includes(t)) return true;
  return false;
}
