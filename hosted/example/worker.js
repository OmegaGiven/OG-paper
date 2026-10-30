// A whole hosted OG Paper in one Worker, for trying it locally
// (`npx wrangler dev` here) and as the template a site copies: the web app
// at /paper/, its API at /paper/api/, one room per page.
//
// Accounts here are pretend (DEV ONLY): /dev/login?as=alice signs in as
// "alice" with a cookie. A real site passes its own adapter instead (see
// ../adapters/omegagiven.js).

import { createPaperApi, PaperRoom } from '../src/index.js';

export { PaperRoom };

const who = req => /(?:^|;\s*)dev_user=([A-Za-z0-9_]{1,20})/.exec(req.headers.get('Cookie') || '')?.[1] || null;

const paper = createPaperApi({
  auth: {
    user: req => (who(req) ? { id: who(req), name: who(req) } : null),
    find: (_env, name) => (/^[A-Za-z0-9_]{1,20}$/.test(name) ? { id: name, name } : null),
    names: (_env, ids) => Object.fromEntries(ids.map(i => [i, i])),
    isAdmin: u => u.id === 'admin',
  },
  onReport: r => console.log('og-paper report', JSON.stringify(r)),
  // Where "Get more storage" goes (a real site: its billing page).
  config: { upgradeUrl: '/dev/storage' },
});

export default {
  async fetch(req, env, ctx) {
    const url = new URL(req.url);
    if (url.pathname === '/dev/login') {
      const as = url.searchParams.get('as') || '';
      const ok = /^[A-Za-z0-9_]{1,20}$/.test(as);
      return new Response(null, {
        status: 302,
        headers: { Location: url.searchParams.get('next') || '/paper/', 'Set-Cookie': `dev_user=${ok ? as : ''}; Path=/; HttpOnly; SameSite=Lax${ok ? '' : '; Max-Age=0'}` },
      });
    }
    if (url.pathname === '/dev/storage') {
      return new Response('<!doctype html><title>More storage</title><h1>More storage</h1><p>Paid cloud storage plans would be sold here.</p>', { headers: { 'content-type': 'text/html' } });
    }
    const api = await paper(req, env, ctx);
    if (api) return api;
    // The web app (web/app/ in this repository) at /paper/, told where its API is.
    if (url.pathname === '/paper') return Response.redirect(url.origin + '/paper/', 301);
    if (url.pathname.startsWith('/paper/')) {
      const asset = new URL(url);
      asset.pathname = '/app/' + url.pathname.slice('/paper/'.length);
      const res = await env.ASSETS.fetch(new Request(asset, req));
      if (!(res.headers.get('content-type') || '').includes('text/html')) return res;
      return new HTMLRewriter().on('head', {
        element(e) {
          e.append('<meta name="og-paper-api" content="/paper/api"><meta name="og-paper-signin" content="/dev/login?as=alice&next={next}">', { html: true });
        },
      }).transform(res);
    }
    return env.ASSETS.fetch(req);
  },
};
