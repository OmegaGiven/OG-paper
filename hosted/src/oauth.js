// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// Signing in from the native apps (see docs/NATIVE-SIGN-IN.md). The app is a
// public OAuth client with PKCE: it opens the system browser at /oauth/authorize,
// the site signs the person in as it always does, the person agrees, and the
// browser returns to the app with a one-time code the app swaps for tokens.
// Passwords and Google, GitHub or Apple tokens never reach the app.
//
// Tokens are random and only their hashes are kept, so a grant can be revoked at
// once. Access tokens last an hour; refresh tokens rotate on every use. Using an
// old refresh token means it was copied, so the grant ends, except for a short
// grace in which an app that lost the answer on a bad network may ask again.

const enc = new TextEncoder();
const b64url = buf => btoa(String.fromCharCode(...new Uint8Array(buf))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
const sha256 = async s => b64url(await crypto.subtle.digest('SHA-256', enc.encode(s)));
const random = () => b64url(crypto.getRandomValues(new Uint8Array(32)));
const esc = s => String(s).replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);

export const APP_CLIENT = 'og-paper-app';
const CODE_MS = 5 * 60_000;
const ACCESS_MS = 3_600_000;
const REFRESH_MS = 180 * 86_400_000;
const GRACE_MS = 60_000;
const USED_EVERY_MS = 3_600_000;
const CHALLENGE = /^[A-Za-z0-9_-]{43,128}$/;
const VERIFIER = /^[A-Za-z0-9._~-]{43,128}$/;

/** The app's custom scheme (phones, Mac) or a loopback port (Windows, Linux). */
export function redirectOk(uri) {
  return uri === 'ogpaper://signed-in' || /^http:\/\/127\.0\.0\.1:[0-9]{1,5}\/signed-in$/.test(String(uri));
}

/** The token in `Authorization: Bearer`, if the request carries one. */
export function bearerToken(req) {
  const m = /^Bearer +(\S+)$/i.exec(req.headers.get('Authorization') || '');
  return m ? m[1] : null;
}

/** The live grant a token belongs to, or null. */
export async function grantOf(db, token, now = Date.now()) {
  const g = await db.prepare('SELECT * FROM paper_app_grants WHERE access_hash = ?1').bind(await sha256(token)).first();
  if (!g || g.access_expires < now) return null;
  if (g.last_used < now - USED_EVERY_MS) {
    await db.prepare('UPDATE paper_app_grants SET last_used = ?1 WHERE id = ?2').bind(now, g.id).run();
  }
  return g;
}

async function issue(db, grant, now, rotating) {
  const access = 'ogpa_' + random(), refresh = 'ogpr_' + random();
  const [ah, rh] = [await sha256(access), await sha256(refresh)];
  if (rotating) {
    await db.prepare(`UPDATE paper_app_grants SET access_hash = ?1, access_expires = ?2, prev_refresh_hash = refresh_hash,
      refresh_hash = ?3, rotated_at = ?4, refresh_expires = ?5, last_used = ?4 WHERE id = ?6`)
      .bind(ah, now + ACCESS_MS, rh, now, now + REFRESH_MS, grant).run();
  } else {
    await db.prepare(`UPDATE paper_app_grants SET access_hash = ?1, access_expires = ?2, refresh_hash = ?3,
      refresh_expires = ?4 WHERE id = ?5`).bind(ah, now + ACCESS_MS, rh, now + REFRESH_MS, grant).run();
  }
  return { access_token: access, token_type: 'Bearer', expires_in: ACCESS_MS / 1000, refresh_token: refresh };
}

const oauthError = (json, error, status = 400) => json({ error }, status);

function page(body, redirect) {
  const to = redirect ? ' ' + (redirect.startsWith('ogpaper:') ? 'ogpaper:' : new URL(redirect).origin) : '';
  return new Response(`<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Sign in to OG Paper</title><style>body{font:16px/1.5 system-ui,sans-serif;background:#f4f1ea;color:#2d2d37;margin:0;display:grid;place-items:center;min-height:100dvh}
main{background:#fff;border-radius:14px;padding:28px;max-width:360px;margin:16px;box-shadow:0 2px 12px #0002}h1{font-size:22px;margin:0 0 12px}
form{display:flex;gap:8px;margin-top:20px}button{flex:1;font:600 16px system-ui;padding:12px;border-radius:10px;border:1px solid #ccc;background:#fff;color:inherit}
button[value=allow]{background:#2d2d37;color:#fff;border-color:#2d2d37}</style><main>${body}</main></html>`, {
    status: 200,
    headers: {
      'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store', 'x-frame-options': 'DENY',
      'content-security-policy': `default-src 'none'; style-src 'unsafe-inline'; form-action 'self'${to}; frame-ancestors 'none'`,
    },
  });
}

/** Answer /oauth/*, or null when the path is something else. */
export async function oauthRoutes({ path, req, url, db, cfg, user, grant, json, fail, revoke }) {
  if (path === '/oauth/authorize') return authorize();
  if (path === '/oauth/token' && req.method === 'POST') return token();
  if (path === '/oauth/revoke' && req.method === 'POST') {
    if (!grant) fail(401, 'Sign in again');
    await revoke(grant.id, grant.user_id);
    return json({ ok: true });
  }
  return null;

  async function authorize() {
    if (req.method !== 'GET' && req.method !== 'POST') fail(405, 'Use GET or POST');
    if (grant) fail(403, 'Sign in through the website');
    const q = req.method === 'POST' ? new URLSearchParams(await req.text()) : url.searchParams;
    const redirect = q.get('redirect_uri') || '';
    const challenge = q.get('code_challenge') || '';
    // Nothing is sent back to an address that isn't the app's own.
    if (q.get('client_id') !== APP_CLIENT || !redirectOk(redirect)) fail(400, 'This sign-in link is not valid');
    if (!CHALLENGE.test(challenge) || q.get('code_challenge_method') !== 'S256') fail(400, 'This sign-in link is not valid');
    const state = (q.get('state') || '').slice(0, 128);
    const device = (q.get('device') || 'this device').replace(/[\u0000-\u001f]/g, ' ').trim().slice(0, 40) || 'this device';
    if (!user) {
      if (req.method === 'POST') fail(401, 'Sign in first');
      const next = url.pathname + url.search;
      return new Response(null, { status: 302, headers: { location: cfg.signIn.replace('{next}', encodeURIComponent(next)), 'cache-control': 'no-store' } });
    }
    const back = params => {
      const to = new URL(redirect);
      for (const [k, v] of Object.entries({ ...params, state, iss: url.origin })) if (v) to.searchParams.set(k, v);
      return new Response(null, { status: 302, headers: { location: to.href, 'cache-control': 'no-store' } });
    };
    if (req.method === 'POST') {
      if (q.get('choice') !== 'allow') return back({ error: 'access_denied' });
      const code = random();
      const now = Date.now();
      await db.prepare('DELETE FROM paper_app_codes WHERE expires_at < ?1').bind(now).run();
      await db.prepare(`INSERT INTO paper_app_codes (code_hash, user_id, redirect_uri, challenge, device, expires_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)`).bind(await sha256(code), user.id, redirect, challenge, device, now + CODE_MS).run();
      return back({ code });
    }
    // Always asked, so another program can't start this and sign itself in unseen.
    const hidden = Object.entries({ client_id: APP_CLIENT, redirect_uri: redirect, code_challenge: challenge, code_challenge_method: 'S256', state, device })
      .map(([k, v]) => `<input type="hidden" name="${k}" value="${esc(v)}">`).join('');
    return page(`<h1>Sign in to OG Paper?</h1><p>The OG Paper app on <b>${esc(device)}</b> will be able to open and change your pages as <b>${esc(user.name || 'you')}</b>.</p>
<p>Only agree if you just started this from the app.</p><form method="post">${hidden}<button name="choice" value="deny">Cancel</button><button name="choice" value="allow">Sign in</button></form>`, redirect);
  }

  async function token() {
    const q = new URLSearchParams(await req.text());
    if (q.get('client_id') !== APP_CLIENT) return oauthError(json, 'invalid_client', 401);
    const now = Date.now();
    if (q.get('grant_type') === 'authorization_code') {
      const hash = await sha256(q.get('code') || '');
      const c = await db.prepare('SELECT * FROM paper_app_codes WHERE code_hash = ?1').bind(hash).first();
      // One use, right or wrong: a code that was tried is gone.
      if (c) await db.prepare('DELETE FROM paper_app_codes WHERE code_hash = ?1').bind(hash).run();
      const verifier = q.get('code_verifier') || '';
      if (!c || c.expires_at < now || c.redirect_uri !== q.get('redirect_uri') || !VERIFIER.test(verifier)
        || await sha256(verifier) !== c.challenge) return oauthError(json, 'invalid_grant');
      const id = random().slice(0, 24);
      await db.prepare(`INSERT INTO paper_app_grants (id, user_id, device, access_hash, access_expires, refresh_hash, refresh_expires, created_at, last_used)
        VALUES (?1, ?2, ?3, ?4, 0, ?5, 0, ?6, ?6)`).bind(id, c.user_id, c.device, 'new:' + id, 'new:' + id, now).run();
      return json(await issue(db, id, now, false));
    }
    if (q.get('grant_type') === 'refresh_token') {
      const hash = await sha256(q.get('refresh_token') || '');
      const g = await db.prepare('SELECT * FROM paper_app_grants WHERE refresh_hash = ?1').bind(hash).first();
      if (g) {
        if (g.refresh_expires < now) return oauthError(json, 'invalid_grant');
        return json(await issue(db, g.id, now, true));
      }
      const old = await db.prepare('SELECT * FROM paper_app_grants WHERE prev_refresh_hash = ?1').bind(hash).first();
      if (old) {
        if (old.rotated_at >= now - GRACE_MS) return json(await issue(db, old.id, now, false));
        await revoke(old.id, old.user_id);
      }
      return oauthError(json, 'invalid_grant');
    }
    return oauthError(json, 'unsupported_grant_type');
  }
}
