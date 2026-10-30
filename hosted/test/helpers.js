import assert from 'node:assert/strict';
import { HostedClient } from '../client/oplog.js';
import { keys } from './fakes.js';

export async function newPage(s, as, name = 'A page') {
  const r = await s.call(as, 'POST', '/pages', { name, ...keys() });
  assert.equal(r.status, 201, JSON.stringify(r.body));
  return r.body.page.id;
}

let clock = 1_000_000;
/** A shared fake wall clock, so stamps are ordered as the test does things. */
export const tick = () => ++clock;

export async function client(s, as, page, opts = {}) {
  const c = new HostedClient({ peer: `peer-${as}-${Math.random().toString(36).slice(2, 7)}`, dial: s.dial(as, page), now: opts.now || tick, tiles: opts.tiles ?? null });
  if (opts.connect !== false) assert.ok(await c.connect(), 'connects: ' + JSON.stringify(c.refused));
  return c;
}

export const sorted = set => [...set].sort();
export const last = (c, t) => [...c.notes].reverse().find(n => n.t === t);
