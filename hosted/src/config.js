// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// Limits and switches of a hosted OG Paper. The host site overrides any of
// them (createPaperApi({ config }) and the PAPER_* variables, see HOSTED.md).

const MB = 1024 * 1024;

export const DEFAULTS = {
  /** Where the API is mounted on the host site. */
  prefix: '/paper/api',
  /** Bytes each free account may keep (pages, pictures, history). */
  freeQuota: 25 * MB,
  /** Pages one account may own. */
  maxPages: 100,
  /** People a page may be shared with (owners included). */
  maxMembers: 25,
  /** Live links per page. */
  maxLinks: 5,
  /** Link sharing at all ("anyone with the link"). */
  linkSharing: true,
  /** Whether a link works without signing in (view only, never edit). */
  linkAnonymousView: false,
  /** The largest kept frame (a change, or pictures sent inline). */
  maxOpBytes: 1 * MB,
  /** The largest passed-on frame (a view, a stroke being drawn). */
  maxEphBytes: 16 * 1024,
  /** The largest full copy (checkpoint). */
  maxCheckpointBytes: 25 * MB,
  /** Kept frames per room before a checkpoint is asked for. */
  maxFramesBeforeCheckpoint: 5000,
  /** Sockets one room takes at once. */
  maxSockets: 32,
  /** Tiles one socket may subscribe to. */
  maxSubs: 512,
  /** Per socket: frames per second (sustained) and burst. */
  frameRate: 60,
  frameBurst: 240,
  /** Per socket: bytes per second (sustained) and burst. */
  byteRate: 512 * 1024,
  byteBurst: 4 * MB,
  /** Quota taken from the account at a time by a busy room (see quota.js). */
  leaseChunk: 256 * 1024,
  /** A page's name. */
  maxName: 120,
  /** Page names and report reasons are trimmed to this. */
  maxReason: 2000,
  /** Kept frames above this size go to R2 (when bound) or are split. */
  chunkBytes: 1 * MB,
};

export function configFrom(env = {}, over = {}) {
  const c = { ...DEFAULTS, ...over };
  const num = (k, v) => { const n = Number(v); if (v != null && v !== '' && Number.isFinite(n) && n >= 0) c[k] = n; };
  num('freeQuota', env.PAPER_FREE_QUOTA);
  num('maxPages', env.PAPER_MAX_PAGES);
  num('maxOpBytes', env.PAPER_MAX_OP);
  if (env.PAPER_LINK_SHARING != null) c.linkSharing = !/^(0|off|false|no)$/i.test(String(env.PAPER_LINK_SHARING));
  if (env.PAPER_LINK_ANON != null) c.linkAnonymousView = /^(1|on|true|yes)$/i.test(String(env.PAPER_LINK_ANON));
  return c;
}
