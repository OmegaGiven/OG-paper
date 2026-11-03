// Launch-audit probes (docs/LAUNCH-AUDIT.md): what another account, a
// viewer or an editor can do to a page that is not theirs, against
// `wrangler dev` (real workerd, Durable Objects, D1, WebSockets).
//   cd hosted/example && npx wrangler d1 migrations apply DB --local && npx wrangler dev --port 8799 &
//   BASE=http://localhost:8799 node hosted/test/audit-probe.mjs
//
// It prints one line per probe: PASS (behaves safely) or FINDING (see the
// audit for severity). It never exits non-zero on a FINDING: it documents.

import { TAG, withTag, READY } from '../src/protocol.js';

const BASE = process.env.BASE || 'http://localhost:8799';
const WS = BASE.replace(/^http/, 'ws');
const wait = ms => new Promise(r => setTimeout(r, ms));
const results = [];
const report = (ok, name, detail = '') => { results.push({ ok, name }); console.log(`${ok ? 'PASS   ' : 'FINDING'} ${name}${detail ? ' :: ' + detail : ''}`); };

async function login(as) {
  const r = await fetch(`${BASE}/dev/login?as=${as}`, { redirect: 'manual' });
  return r.headers.get('set-cookie').split(';')[0];
}
async function call(cookie, method, path, body, headers = { Origin: BASE }) {
  const r = await fetch(`${BASE}/paper/api${path}`, {
    method, headers: { Cookie: cookie, ...headers, ...(body ? { 'content-type': 'application/json' } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  let b = null; try { b = await r.json(); } catch (e) {}
  return { status: r.status, body: b };
}
function sock(cookie, page) {
  return new Promise(ok => {
    const w = new WebSocket(`${WS}/paper/api/pages/${page}/relay`, { headers: { Cookie: cookie, Origin: BASE } });
    w.binaryType = 'arraybuffer';
    const s = { w, got: [], text: [], closed: false };
    w.onopen = () => ok(s);
    w.onerror = () => ok(null);
    w.onmessage = e => { if (typeof e.data === 'string') s.text.push(JSON.parse(e.data)); else s.got.push(new Uint8Array(e.data)); };
    w.onclose = () => { s.closed = true; };
  });
}
const isReady = b => b.length === READY.length && b.every((x, i) => x === READY[i]);
const until = async (f, ms = 4000) => { const t = Date.now(); while (!f()) { if (Date.now() - t > ms) return false; await wait(20); } return true; };
let n = 0;
const keys = () => { n++; return { canvas: (Date.now().toString(16) + n).padStart(32, '0').slice(-32), edit: 'e' + 'A'.repeat(39) + String(n).padStart(4, '0'), view: 'v' + 'B'.repeat(82) + String(n).padStart(4, '0') }; };
const ROOMID = new Uint8Array(32).fill(7);
async function joined(cookie, page, room = ROOMID) {
  const s = await sock(cookie, page);
  if (!s) return null;
  s.w.send(withTag(TAG.ROOM, room));
  await until(() => s.got.some(isReady) || s.closed);
  return s;
}

const run = Date.now().toString(36).slice(-6);
const [A, B, C, M] = ['alice', 'bob', 'carol', 'mallory'].map(x => `${x}_${run}`);
const alice = await login(A), bob = await login(B), carol = await login(C), mallory = await login(M);

// ---- 1. IDOR: a stranger and a page id ---------------------------------------
const pg = (await call(alice, 'POST', '/pages', { name: 'Alice private', ...keys() })).body.page.id;
const probes = [
  ['GET', `/pages/${pg}`], ['PATCH', `/pages/${pg}`, { name: 'pwned' }], ['DELETE', `/pages/${pg}`],
  ['POST', `/pages/${pg}/open`], ['POST', `/pages/${pg}/copy`, keys()], ['POST', `/pages/${pg}/members`, { who: M, role: 'owner' }],
  ['POST', `/pages/${pg}/links`, { role: 'editor' }], ['POST', `/pages/${pg}/report`, { reason: 'x' }],
  ['POST', `/pages/${pg}/transfer`, { to: M }], ['DELETE', `/pages/${pg}/members/${A}`], ['PATCH', `/pages/${pg}/members/${A}`, { role: 'viewer' }],
  ['GET', '/admin/reports'], ['POST', `/admin/accounts/${M}`, { quota: 1e12 }],
];
const leaks = [];
for (const [m, p, b] of probes) { const r = await call(mallory, m, p, b); if (r.status !== 404) leaks.push(`${m} ${p} -> ${r.status}`); }
report(leaks.length === 0, 'IDOR: a stranger gets 404 on every page/admin route', leaks.join('; '));
report(!(await sock(mallory, pg)), 'IDOR: a stranger cannot open the live room');

// ---- 2. Viewer / editor limits ---------------------------------------------------
await call(alice, 'POST', `/pages/${pg}/members`, { who: B, role: 'editor' });
await call(alice, 'POST', `/pages/${pg}/members`, { who: C, role: 'viewer' });
const vr = await call(carol, 'PATCH', `/pages/${pg}`, { name: 'viewer rename' });
report(vr.status === 403, 'viewer cannot rename', String(vr.status));
const er = await call(bob, 'POST', `/pages/${pg}/members`, { who: M, role: 'editor' });
report(er.status === 403, 'editor cannot share', String(er.status));
const ed = await call(bob, 'DELETE', `/pages/${pg}`);
report(ed.status === 403, 'editor cannot delete', String(ed.status));

// ---- 3. Stored names are raw HTML (the client must escape every use) -----------
const xs = await call(alice, 'PATCH', `/pages/${pg}`, { name: '<img src=x onerror=alert(1)>' });
report(xs.status === 200 && !/[<>]/.test(xs.body?.name || ''), 'page names lose markup characters server-side', `rename -> ${xs.status} ${JSON.stringify(xs.body?.name)}`);

// ---- 4. Live room: an editor can wipe the page's history with a tiny checkpoint --
const sa = await joined(alice, pg);
for (let i = 0; i < 5; i++) sa.w.send(withTag(TAG.PUT, new Uint8Array(200).fill(i + 1)));
await until(() => sa.text.filter(m => m.t === 'kept').length >= 5);
const sb = await joined(bob, pg);
sb.w.send(withTag(TAG.CKPT, new Uint8Array(8)));
await until(() => sb.text.some(m => m.t === 'kept' || m.t === 'ack'));
const late = await joined(carol, pg);
const kept = late.got.filter(b => !isReady(b));
report(kept.length > 1, 'an editor cannot replace the server copy with an empty checkpoint', `a new viewer now receives ${kept.length} frame(s) of ${8} bytes; 5 x 200 B changes gone from the server`);

// ---- 5. A viewer's passed-on (EPH) frames reach editors -------------------------
const before = sa.got.length;
late.w.send(withTag(TAG.EPH, new Uint8Array(64).fill(9)));
const eph = await until(() => sa.got.length > before, 1500);
report(!eph, 'a viewer cannot push frames to editors', eph ? 'EPH from a viewer is passed on (sealed, so only presence/strokes-in-progress the app accepts; spoofing depends on the app checking signatures on EPH)' : '');
for (const s of [sa, sb, late]) s.w.close();

// ---- 6. A viewer can pin a fresh page to a bogus room before the owner joins -----
const fresh = (await call(alice, 'POST', '/pages', { name: 'Fresh', ...keys() })).body.page.id;
await call(alice, 'POST', `/pages/${fresh}/members`, { who: C, role: 'viewer' });
const pin = await joined(carol, fresh, new Uint8Array(32).fill(66));
pin.w.close();
const own = await sock(alice, fresh);
own.w.send(withTag(TAG.ROOM, ROOMID));
await until(() => own.closed || own.got.some(isReady));
report(!own.closed, 'a viewer cannot pin a new page to the wrong room key', own.closed ? 'owner refused: "This link\'s key does not belong to this page" (page unusable until an admin clears it)' : '');
if (!own.closed) own.w.close();

// ---- 7. Give away without the receiver agreeing ---------------------------------
const gift = (await call(mallory, 'POST', '/pages', { name: 'Unwanted gift', ...keys() })).body.page.id;
const gs = await joined(mallory, gift);
gs.w.send(withTag(TAG.PUT, new Uint8Array(50_000).fill(1)));
await until(() => gs.text.some(m => m.t === 'kept'));
gs.w.close();
const tr = await call(mallory, 'POST', `/pages/${gift}/transfer`, { who: B, leave: true });
const offered = (await call(bob, 'GET', '/pages')).body.offers?.some(o => o.page.id === gift);
report(tr.status === 201 && offered, 'giving away makes an offer the recipient sees', `transfer -> ${tr.status}; offered: ${offered}`);
const bl = await call(bob, 'GET', '/pages');
const landed = bl.body.mine.some(p => p.id === gift);
report(!landed, 'a page cannot be pushed into another account (and its quota) without consent', `transfer -> ${tr.status}; in bob's My pages: ${landed}; bob used ${bl.body.usage.used} B`);
const ghost = (await call(mallory, 'POST', '/pages', { name: 'To nobody', ...keys() })).body.page.id;
const tg = await call(mallory, 'POST', `/pages/${ghost}/transfer`, { to: 'no_such_user_' + run, leave: true });
report(tg.status !== 200, 'transfer refuses account ids that do not exist', `transfer to a made-up id -> ${tg.status} (page orphaned)`);

// ---- 8. CSRF: writes without Origin / Sec-Fetch-Site --------------------------
const nr = await call(alice, 'POST', '/pages', { name: 'no origin', ...keys() }, {});
report(nr.status !== 201, 'writes need an Origin or Sec-Fetch-Site header', `POST with neither -> ${nr.status} (SameSite=Lax cookie is the remaining CSRF guard)`);
const bad = await call(alice, 'POST', '/pages', { name: 'evil', ...keys() }, { Origin: 'https://evil.example' });
report(bad.status === 403, 'writes from another origin are refused', String(bad.status));

// ---- 9. Links outlive the co-owner who made them --------------------------------
const lp = (await call(alice, 'POST', '/pages', { name: 'Links', ...keys() })).body.page.id;
await call(alice, 'POST', `/pages/${lp}/members`, { who: B, role: 'owner' });
const link = (await call(bob, 'POST', `/pages/${lp}/links`, { role: 'editor' })).body;
await call(alice, 'DELETE', `/pages/${lp}/members/${B}`);
const use = await call(mallory, 'POST', `/links/${link.token}`);
report(use.status !== 200, 'removing a co-owner turns off the links they made', `link made by removed co-owner still grants ${use.body?.open?.role} -> ${use.status}`);

// ---- 10. A co-owner can remove the other co-owners -----------------------------
const cp = (await call(alice, 'POST', '/pages', { name: 'Co', ...keys() })).body.page.id;
await call(alice, 'POST', `/pages/${cp}/members`, { who: B, role: 'owner' });
await call(alice, 'POST', `/pages/${cp}/members`, { who: C, role: 'owner' });
const co = await call(bob, 'DELETE', `/pages/${cp}/members/${C}`);
report(co.status !== 200, 'a co-owner cannot remove another co-owner', `-> ${co.status} (by design today; billing owner is protected)`);

console.log(`\n${results.filter(r => r.ok).length} pass, ${results.filter(r => !r.ok).length} findings`);
process.exit(0);
