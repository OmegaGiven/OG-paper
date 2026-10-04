# UI tests (and demos)

Each file in `features/` is one feature (or a few) exercised in the real web
app, in headless Brave or Chrome, at desktop and phone sizes. The same file
is the demo: `--demo` plays it at a human pace with a drawn cursor, ripples
on taps and captions, and records a video.

```sh
scripts/build-web.sh && cargo build -p og-paper    # what the tests run
node tests/ui/run.mjs                              # all tests, both sizes
node tests/ui/run.mjs bookmarks --size=phone       # one test, one size
node tests/ui/run.mjs --demo                       # out/demos/<test>-<size>.mp4
UI_VERBOSE=1 node tests/ui/run.mjs menu            # print every step
```

Results: `out/ui-report.md`, screenshots in `out/shots/`. Demo videos are
1920 x 1200 (desktop) and 1080 x 1920 (phone: Shorts, TikTok, Reels), 30 fps.

## Writing one

```js
export default {
  name: 'bookmarks',                  // the file's name
  features: ['NAV-04'],               // its rows in FEATURES.md
  sizes: ['desktop', 'phone'],
  title: 'Bookmarks: save a view',    // the demo's opening caption
  async run(t) {
    await t.open();                   // the app, nothing saved
    await t.caption('Menu › Bookmarks');   // a caption in the demo
    await t.tap('Menu');              // widgets by name (see below)
    await t.tap('Bookmarks');
    await t.tap(await t.el('.og-card input'));  // page elements by selector
    await t.type('Start');
    await t.wait(async () => (await t.state()).bookmarks.length === 1, 'the bookmark is saved');
    await t.shot('saved');
  },
};
```

Widgets are found by the names the app gives them (its accessibility tree:
buttons by their text, text boxes by their hint, hand-drawn buttons named
in `ui.rs` with `named()`), so tests keep working when the layout moves.
`t.state()` is the app's status (zoom, strokes, undos, bookmarks, name,
net, ...). `startHub()` runs a page server for workspace and sharing tests.

Then add the test to its features' rows in `FEATURES.md` (`ui:<name>`).
