import { expect, test } from 'vitest';
import { pokeKnob, readSectors } from '@digi/regression/runtime/card-kit.ts';
import { SETTLE } from '@digi/regression/runtime/fx-live.ts';
import { pdwSuite, pattern, slot4, cat, drive, staged, WRITE, ABORT, SAMPLES, HASH_PAGES, IDLE_MS } from './plusdrive-write-support.ts';

// +Drive writes through core/plusdrive-write, the close hash (docs/plans/usb-fast.md 3.1, finding 120; setup: plusdrive-write-support.ts).

pdwSuite('+Drive writes through core/plusdrive-write: the close hash', { devices: ['dt2'] }, (t) => {
  const { rig, baseCard, run, snapshot, same, fsWrites, hashTable } = t;
  const bothHashes = async (knob: number) => {
    const dk = await rig.pausedBoot(await baseCard());
    await rig.runtime.ui.wait({ ms: 2000, why: SETTLE });
    const initial = await hashTable();
    await pokeKnob(rig.session, rig.plan, 'DKRT_PD_CUT', knob);
    const a = pattern(70000); // two pages and a tail
    const b = pattern(5000); // one partial page; the second file's table holds both
    const c = pattern(32752); // data + 16 zeros is exactly one page
    const r: string[] = [];
    for (const [d, p] of [[a, 'Samples/Kits/a.bin'], [b, 'Samples/b.bin'], [c, 'Samples/c.bin']] as const) {
      const x = await run(dk, WRITE | SAMPLES, d.length, p);
      expect([x.close, x.job, x.devHash], `${p} closed with the hash of data + 16 zeros`).toEqual([0, 0, slot4(d)]);
      r.push(x.devHash);
    }
    const card = await rig.after(`pdh${knob}`);
    const files = await drive(card);
    for (const [d, p] of [[a, 'Samples/Kits/a.bin'], [b, 'Samples/b.bin'], [c, 'Samples/c.bin']] as const) {
      expect(files.get(p), p).toEqual({ size: d.length + 16, hash: slot4(d) });
      expect((await cat(card, p)).subarray(0, d.length), `${p} bytes`).toEqual(d);
    }
    // the card as the three closes left it, before the end of the batch (the staging folder is still there)
    const writes = fsWrites(card);
    const during = await hashTable();
    await rig.runtime.ui.wait({ ms: IDLE_MS, why: SETTLE }); // the runtime's idle point: the rebuild of the table (and the removal of the staging folder)
    return { writes, initial, during, table: await hashTable(), hashes: [a, b, c].map((d) => parseInt(slot4(d), 16)) };
  };

  test('the close hash with the adapter\'s own code leaves the card and the RAM hash table as the firmware\'s close hash does', async () => {
    const fast = await bothHashes(0);
    const slow = await bothHashes(0x80);
    expect(fast.table[0], 'the hash table has entries').toBeGreaterThan(3);
    expect(slow.during, 'the firmware close rebuilds at every file').toEqual(slow.table);
    expect(fast.during[0], 'the batch has not rebuilt the table: it lacks the three new files').toBe(fast.table[0]! - 3);
    expect(fast.during, 'before the idle point the table is the one the boot built').toEqual(fast.initial);
    // a lookup by hash (the firmware's FUN_4015ac28 searches these pairs): the new files are found after the rebuild only
    const hashWords = (t: number[]) => t.slice(1).filter((_, i) => i % 2 === 0).map((w) => w >>> 0);
    for (const h of fast.hashes) {
      expect(hashWords(fast.during).some((w) => (w >>> 1) === (h >>> 1)), `hash ${h.toString(16)} is not in the table yet`).toBe(false);
      expect(hashWords(fast.table).some((w) => (w >>> 1) === (h >>> 1)), `hash ${h.toString(16)} is in the table after the rebuild`).toBe(true);
    }
    expect(fast.table, 'the RAM hash table after the rebuild at the idle point').toEqual(slow.table);
    expect([...fast.writes.keys()].sort((x, y) => x - y), 'the same sectors were written').toEqual([...slow.writes.keys()].sort((x, y) => x - y));
    expect(fast.writes.size, 'the card was written').toBeGreaterThan(20);
    for (const [sector, data] of fast.writes) expect(data, `sector 0x${sector.toString(16)}`).toBe(slow.writes.get(sector));
    await rig.session.expectNoFault('close hash');
  }, 1_800_000);

  test('a structural failure of the fast close hash (knob 10) falls back to the firmware\'s hash and leaves the same card and table', async () => {
    const fallback = await bothHashes(10);
    const slow = await bothHashes(0x80);
    expect(fallback.table, 'the RAM hash table after the rebuild').toEqual(slow.table);
    expect([...fallback.writes.keys()].sort((x, y) => x - y), 'the same sectors were written').toEqual([...slow.writes.keys()].sort((x, y) => x - y));
    for (const [sector, data] of fallback.writes) expect(data, `sector 0x${sector.toString(16)}`).toBe(slow.writes.get(sector));
    await rig.session.expectNoFault('close hash fallback');
  }, 1_800_000);

  test('an abort in the middle of a batch rebuilds the RAM hash table at once', async () => {
    const dk = await rig.pausedBoot(await baseCard());
    await rig.runtime.ui.wait({ ms: 2000, why: SETTLE });
    const initial = await hashTable();
    const a = pattern(40000);
    const x = await run(dk, WRITE | SAMPLES, a.length, 'Samples/batch.bin');
    expect([x.close, x.job]).toEqual([0, 0]);
    expect(await hashTable(), 'the close left the table as it was').toEqual(initial);
    const ab = await run(dk, ABORT | SAMPLES, 20000, 'Samples/aborted.bin'); // a second file of the batch is dropped before its close
    expect([ab.abort, ab.job], 'the session was aborted').toEqual([0, 0]);
    const t = await hashTable();
    expect(t[0], 'the abort rebuilt the table: it holds the first file').toBe(initial[0]! + 1);
    expect(t.slice(1).filter((_, i) => i % 2 === 0).some((w) => (w >>> 1) === (parseInt(slot4(a), 16) >>> 1))).toBe(true);
    await rig.session.expectNoFault('abort rebuild');
  }, 1_800_000);

  test('a failure after the hash page was written puts the hash word back: nothing points at the deleted inode', async () => {
    const dk = await rig.pausedBoot(await baseCard());
    await rig.runtime.ui.wait({ ms: 2000, why: SETTLE });
    const before = await snapshot('pdw0');
    const tableBefore = await hashTable();
    await pokeKnob(rig.session, rig.plan, 'DKRT_PD_CUT', 9); // the close fails right after the hash page write
    const r = await run(dk, WRITE | SAMPLES, 40000, 'Samples/never.bin');
    expect(r.job, 'write error').toBe(-57);
    const after = await snapshot('pdw1');
    expect(fsWrites(after.card).has(HASH_PAGES[0]!), 'the hash page was written (and written back)').toBe(true);
    const pages = (card: string) => readSectors(card, HASH_PAGES[0]!, HASH_PAGES[1]! - HASH_PAGES[0]!);
    expect(pages(after.card).equals(pages(before.card)), 'every hash word is as it was').toBe(true);
    expect(await hashTable(), 'the RAM hash table was not rebuilt').toEqual(tableBefore);
    expect(after.files.has('Samples/never.bin')).toBe(false);
    expect(staged(after.files)).toEqual([]);
    same(before.files, after.files, []);
    await pokeKnob(rig.session, rig.plan, 'DKRT_PD_CUT', 0);
    const ok = await run(dk, WRITE | SAMPLES, 3000, 'Samples/good.bin');
    expect([ok.close, ok.job], 'the next file is accepted').toEqual([0, 0]);
    await rig.session.expectNoFault('hash word restore');
  }, 1_800_000);

});
