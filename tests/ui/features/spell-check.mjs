// Spell check, never autocorrect: the text box underlines misspelled words
// and offers spellings (on the web, the browser's own spell check; the
// apps have their own, see rust:ui::tests). UI settings › Spell check
// turns it off.
export default {
  name: 'spell-check',
  features: ['UI-22'],
  sizes: ['desktop', 'phone'],
  title: 'Spell check in the text box',
  async run(t) {
    await t.open();
    const attrs = () => t.b.eval(`(() => { const a = document.querySelector('.og-text'); return a && [a.spellcheck, a.getAttribute('autocorrect')].join(' '); })()`);
    const typeText = async () => {
      await t.tap('Slot 9');
      await t.tap({ x: t.w * 0.45, y: t.h * 0.4 });
      await t.el('.og-text');
    };
    await t.caption('The text box checks spelling, and never changes a word');
    await typeText();
    t.check(await attrs() === 'true off', `spell check on, autocorrect off (${await attrs()})`);
    await t.type('Teh cat');
    await t.shot('typing');
    await t.key('Escape');

    await t.caption('UI settings › Spell check off');
    await t.tap('Menu');
    const m = await t.find('Search text');
    for (let i = 0; i < 4 && !(await t.has('UI settings').catch(() => false)); i++) await t.wheel(m.x + m.w / 2, m.y + m.h / 2, 500);
    await t.wheel(m.x + m.w / 2, m.y + m.h / 2, 800);
    await t.tap('UI settings');
    await t.tap('Spell check');
    await t.wait(async () => (await t.state()).spell === false, 'spell check off');
    if (await t.has('Close')) await t.tap('Close');
    else if (await t.has('Spell check')) await t.tap('×');
    await typeText();
    t.check(await attrs() === 'false off', `spell check off (${await attrs()})`);
  },
};
