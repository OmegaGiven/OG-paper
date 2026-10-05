// The tools most people use, one after another: brush, texture, highlighter,
// shapes, bucket, text, eraser, select and move, undo, a note deep down,
// home, and the timeline replaying it all.
export default {
  name: 'tools-tour',
  features: ['DRAW-01', 'DRAW-02', 'DRAW-03', 'DRAW-04', 'DRAW-05', 'DRAW-06', 'DRAW-07', 'EDIT-01', 'EDIT-02', 'NAV-01', 'NAV-03', 'NAV-07', 'UI-03'],
  sizes: ['desktop', 'phone'],
  title: 'OG Paper: the tools',
  async run(t) {
    await t.open();
    // The free canvas: right of the toolbar (phone) or tool panel (desktop).
    const x0 = t.phone ? t.w * 0.33 : t.w * 0.3, x1 = t.w - (t.phone ? 20 : 60);
    const y0 = t.h * 0.15, y1 = t.h * (t.phone ? 0.78 : 0.8);
    const X = f => x0 + (x1 - x0) * f, Y = f => y0 + (y1 - y0) * f;
    const wave = (yf, amp = 0.05) => Array.from({ length: 17 }, (_, i) => [X(0.05 + 0.9 * i / 16), Y(yf) + Math.sin(i / 2.2) * (y1 - y0) * amp]);
    const more = async (key, what, before) => t.wait(async () => (await t.state())[key] > before, what, 6000);
    let s = await t.state();

    await t.caption('The brush: pressure, colour, any width');
    await t.tap('Slot 1');
    await t.drag(wave(0.08), 900);
    await more('strokes', 'a brush stroke', s.strokes); s = await t.state();

    await t.caption('The texture brush');
    await t.tap('Slot 2');
    await t.drag(wave(0.24, 0.04), 900);
    await more('strokes', 'a texture stroke', s.strokes); s = await t.state();

    await t.caption('The highlighter: glows, never hides');
    await t.tap('Slot 3');
    await t.drag([[X(0.1), Y(0.09)], [X(0.9), Y(0.09)]], 700);
    await more('strokes', 'a highlight', s.strokes); s = await t.state();

    await t.caption('Shapes');
    await t.tap('Slot 8');
    await t.drag([[X(0.08), Y(0.42)], [X(0.25), Y(0.52)], [X(0.42), Y(0.66)]], 700);
    await more('strokes', 'a shape', s.strokes); s = await t.state();

    await t.caption('The bucket fills a closed shape');
    await t.tap('Slot 4');
    await t.tap({ x: X(0.25), y: Y(0.54) });
    await more('strokes', 'a fill', s.strokes); s = await t.state();

    await t.caption('Text, with real fonts');
    await t.tap('Slot 9');
    await t.tap(t.phone ? { x: X(0.0), y: Y(0.75) } : { x: X(0.55), y: Y(0.5) });
    await t.el('.og-text');
    await t.type('Made with OG Paper');
    await t.key('Enter', 2); // Ctrl+Enter finishes the text
    await more('strokes', 'the text', s.strokes); s = await t.state();
    await t.shot('drawn');

    await t.caption('The eraser');
    await t.tap('Slot 5');
    await t.drag([[X(0.2), Y(0.18)], [X(0.35), Y(0.3)]], 600);
    await more('erased', 'something erased', s.erased); s = await t.state();

    await t.caption('Select and move');
    await t.tap('Slot 6');
    await t.drag([[X(0.04), Y(0.38)], [X(0.46), Y(0.7)]], 600);
    await t.drag([[X(0.25), Y(0.54)], [X(0.3), Y(0.6)], [X(0.35), Y(0.62)]], 600);
    await t.shot('moved');

    await t.caption('Undo and redo lead the tool fan');
    s = await t.state();
    await t.tap('Tools');
    await t.tap('Undo');
    await more('undos', 'undone', s.undos);
    await t.tap('Redo');
    await t.tap('Tools');

    await t.caption('Zoom in as far as you like...');
    const z0 = (await t.state()).zoom;
    await t.tap('Slot 1');
    if (t.phone) { for (let i = 0; i < 3; i++) await t.pinch(X(0.5), Y(0.62), 60, 330); }
    else await t.wheel(X(0.5), Y(0.62), -120, 26);
    await t.wait(async () => (await t.state()).zoom > z0 + 1.5, 'zoomed in deep', 8000);
    await t.caption('...and write a note down there');
    s = await t.state();
    await t.drag([[X(0.35), Y(0.4)], [X(0.35), Y(0.6)], [X(0.35), Y(0.5)], [X(0.5), Y(0.5)], [X(0.5), Y(0.4)], [X(0.5), Y(0.6)]], 900);
    await t.drag([[X(0.62), Y(0.4)], [X(0.62), Y(0.6)]], 400);
    await more('strokes', 'a deep note', s.strokes);
    await t.shot('deep');

    await t.caption('Home flies back');
    await t.tap('Menu');
    await t.tap('Home');
    await t.wait(async () => { const st = await t.state(); return !st.flying && Math.abs(st.zoom) < 0.1; }, 'home', 10000);

    await t.caption('The timeline replays everything');
    await t.tap('Menu');
    await t.tap('Timeline');
    await t.tap(await t.el('.og-tl .play'));
    await t.beat(4000);
    await t.shot('timeline');
    t.check((await t.state()).timeline != null, 'the timeline is open');
  },
};
