// The web app's chat shows messages as text only: links become <a> for
// http(s) only, @mentions of people on the page are marked, and nothing in
// a message is ever parsed as HTML. (A tiny stand-in DOM: no browser.)

import test from 'node:test';
import assert from 'node:assert/strict';
import { renderText, colorOf } from '../../web/app/og-chat.js';

class Node {
  constructor(doc, tag, text) { this.ownerDocument = doc; this.tag = tag; this.kids = []; this.text = text ?? null; this.className = ''; this.attrs = {}; }
  append(...n) { this.kids.push(...n); }
  set textContent(t) { this.kids = [new Node(this.ownerDocument, '#text', t)]; }
  get textContent() { return this.text ?? this.kids.map(k => k.textContent).join(''); }
  set href(v) { this.attrs.href = v; } get href() { return this.attrs.href; }
  set target(v) { this.attrs.target = v; } set rel(v) { this.attrs.rel = v; } get rel() { return this.attrs.rel; }
}
const doc = { createTextNode: t => new Node(doc, '#text', t), createElement: t => new Node(doc, t) };
const render = (text, names = [], me = '') => {
  const n = new Node(doc, 'div');
  const at = renderText(n, text, names, me);
  return { at, parts: n.kids.map(k => k.tag === '#text' ? k.text : k.tag === 'a' ? { a: k.textContent, href: k.href, rel: k.rel } : { [k.className]: k.textContent }) };
};

test('HTML in a message stays text', () => {
  const r = render('<img src=x onerror=alert(1)> <script>alert(1)</script>');
  assert.deepEqual(r.parts, ['<img src=x onerror=alert(1)> <script>alert(1)</script>']);
});

test('http(s) links become links; other schemes do not', () => {
  const r = render('see https://example.com/a?b=1, or javascript:alert(1) or data:text/html,x (http://x.org/p).');
  assert.deepEqual(r.parts, [
    'see ', { a: 'https://example.com/a?b=1', href: 'https://example.com/a?b=1', rel: 'noopener noreferrer nofollow ugc' },
    ', or javascript:alert(1) or data:text/html,x (', { a: 'http://x.org/p', href: 'http://x.org/p', rel: 'noopener noreferrer nofollow ugc' }, ').',
  ]);
  // A link cannot carry a quote out of its attribute (it is set as a property, but still).
  assert.deepEqual(render('https://a.b/"onmouseover=x').parts[0], { a: 'https://a.b/', href: 'https://a.b/', rel: 'noopener noreferrer nofollow ugc' });
});

test('@mentions of people on the page are marked, longest name first', () => {
  const r = render('hey @Ann Lee and @bob, not @nobody or mail@bob', ['Ann', 'Ann Lee', 'bob'], 'BOB');
  assert.equal(r.at, true);
  assert.deepEqual(r.parts, ['hey ', { mention: '@Ann Lee' }, ' and ', { mention: '@bob' }, ', not @nobody or mail@bob']);
  assert.equal(render('hi @bob', ['bob'], 'ann').at, false);
});

test('colours are steady per account', () => {
  assert.equal(colorOf('42'), colorOf('42'));
  assert.notEqual(colorOf('1'), colorOf('2'));
});
