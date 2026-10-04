// Zoom in a long way, then fly Home from the menu.
export default {
  name: 'zoom-home',
  features: ['NAV-01', 'NAV-03'],
  sizes: ['desktop', 'phone'],
  title: 'Endless zoom, and Home',
  async run(t) {
    await t.open();
    await t.scribble(t.w * 0.55, t.h * 0.42, t.w * 0.4);
    await t.caption(t.phone ? 'Pinch to zoom in' : 'Scroll to zoom in');
    const z0 = (await t.state()).zoom;
    await t.zoom(3);
    await t.wait(async () => (await t.state()).zoom > z0 + 0.6, 'the view zooms in');
    const z1 = (await t.state()).zoom;
    t.check(z1 > z0 + 0.6, `zoomed in to 10^${z1.toFixed(1)}`);
    await t.shot('zoomed');
    await t.caption('Menu › Home flies back to where the canvas starts');
    await t.tap('Menu');
    await t.tap('Home');
    await t.wait(async () => Math.abs((await t.state()).zoom) < 0.05 && !(await t.state()).flying, 'the view is back home', 8000);
    t.check(true, 'back at zoom 10^0');
    await t.shot('home');
  },
};
