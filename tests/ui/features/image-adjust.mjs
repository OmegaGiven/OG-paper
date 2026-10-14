// A picture's Brightness, Contrast and Saturation in the selection panel;
// Remove background makes its plain background see-through.
export default {
  name: 'image-adjust',
  features: ['EDIT-15'],
  sizes: ['desktop'],
  title: 'Picture colors and background removal',
  async run(t) {
    await t.open();
    const run = async cmds => {
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
      await t.beat(250);
      return t.b.eval('window.ogPaper.results()');
    };
    await t.caption('Drop a picture: a red square on white');
    await t.b.eval(`(async () => {
      const c = document.createElement('canvas'); c.width = 200; c.height = 200;
      const g = c.getContext('2d'); g.fillStyle = '#ffffff'; g.fillRect(0, 0, 200, 200);
      g.fillStyle = '#e02020'; g.fillRect(50, 50, 100, 100);
      const blob = await new Promise(r => c.toBlob(r, 'image/png'));
      const dt = new DataTransfer(); dt.items.add(new File([blob], 'square.png', { type: 'image/png' }));
      const tgt = document.querySelector('canvas');
      tgt.dispatchEvent(new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true, clientX: ${t.w / 2}, clientY: ${t.h / 2} }));
    })()`);
    await t.wait(async () => (await run([{ get: 'objects', kinds: ['image'] }]))[0].objects.length === 1, 'a picture');
    const o = (await run([{ get: 'objects', kinds: ['image'] }]))[0].objects[0];
    const cx = t.w / 2 + o.x + o.w / 2, cy = t.h / 2 + o.y + o.h / 2;
    const corner = [t.w / 2 + o.x + o.w * 0.08, t.h / 2 + o.y + o.h * 0.08];
    await t.tap('Slot 6');
    await t.tap({ x: cx, y: cy });
    await t.wait(async () => (await t.state()).selected === 1, 'selected');
    if (!(await t.has('Saturation'))) {
      const show = (await t.nodes()).find(n => n.w > 0 && / settings$/.test(n.label || '') && !/^(Hide|Resize|UI|Tool) /.test(n.label));
      if (show) await t.tap(show);
    }
    await t.caption('Saturation to 0: grey');
        const s = await t.find('Saturation', { role: 'Button' });
    await t.tap({ x: s.x + 2, y: s.y + s.h / 2 });
    await t.wait(async () => { const p = await t.pixel(cx, cy); return Math.abs(p[0] - p[1]) < 12; }, 'the red turns grey');
    await t.shot('grey');
    await t.caption('Remove background');
    const before = await t.pixel(...corner);
    t.check(before[0] > 248 && before[1] > 248, `white around it (${before})`);
    await t.tap('Remove background');
    await t.wait(async () => { const p = await t.pixel(...corner); return p[2] < 240; }, 'the paper shows through');
    await t.shot('cut-out');
  },
};
