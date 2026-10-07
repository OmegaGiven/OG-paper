// The hold slot (what a right click or a press and hold uses): one, at the
// end of the main toolbar. Tapping it makes its tool the main one; it only
// changes when empty (a tap saves the current tool) or from the inventory.
export default {
  name: 'hold-slot',
  features: ['UI-03'],
  sizes: ['desktop', 'phone'],
  title: 'The hold slot',
  async run(t) {
    await t.open();
    const st = () => t.state();
    const holds = async () => (await t.nodes()).filter(n => n.w > 0 && [n.label, n.value].includes('Hold slot')).length;
    t.check(await holds() === 1, 'one hold slot');

    await t.caption('Empty, a tap saves the current tool as the hold tool');
    if (!(await st()).hold) {
      await t.tap('Hold slot');
      await t.wait(async () => (await st()).hold === 'Brush', 'the brush is the hold tool');
    }
    const hold = (await st()).hold;

    await t.caption('Pick another tool...');
    await t.tap('Slot 5');
    await t.wait(async () => (await st()).tool !== hold, 'another tool');
    const other = (await st()).tool;

    await t.caption('...then tap the hold slot: its tool becomes the main one');
    await t.tap('Hold slot');
    await t.wait(async () => (await st()).tool === hold, 'the hold tool in hand');
    t.check((await st()).hold === hold, `the hold tool is still ${hold} (not ${other})`);
    await t.shot('hold');
  },
};
