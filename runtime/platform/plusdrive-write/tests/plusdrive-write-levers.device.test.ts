import { expect, test } from 'vitest';
import { pokeKnob, pokeWord } from '@digi/regression/runtime/guest-mailbox.ts';
import { word } from '@digi/regression/runtime/guest-access.ts';
import { SETTLE } from '@digi/regression/runtime/panel-waits.ts';
import { pdwSuite, pattern, slot4, cat, drive, staged, noStagingFolder, WRITE, BIG_CALL, SAMPLES, BITMAP, FRAG_LAST_WORD, IDLE_MS } from './plusdrive-write-support.ts';

// +Drive writes through core/plusdrive-write, the write levers (finding 134) (docs/plans/usb-fast.md 3.1, finding 120; setup: plusdrive-write-support.ts).

pdwSuite('+Drive writes through core/plusdrive-write: the write levers (finding 134)', { devices: ['dt2'] }, (t) => {
  const { rig, settle, baseCard, run, snapshot, fsWrites, hashTable, inodes, fileOfSize, extentsOf } = t;
  // The only part of the fast close that differs from the stock read is the page to card mapping (file_page_find), and a file of one extent does
  // not test it. The reserve is the cheaper one (`reserve_fast`: the page allocator without the free page count): its allocation of a file of 20 pieces
  // gives more extents than the 8 of the inode (an extent index page). The adapter reserves the whole file in one call; the firmware's allocator (FUN_40155974, FUN_4015583c) takes the first free run of
  // that length, else the longest runs it finds, one extent each. So the free space is fragmented in RAM before the write: in the page occupancy
  // bitmap (0x46f39ca0, one bit per 32 KiB page, set = used) every even page from the first one after the seed files is marked used, so that every
  // free run is one page long. Both variants get the same bitmap.
  const FRAG_FIRST_WORD = 83; // pages 2656 and up; the allocator starts at page 2682
  const FRAG_USED_WORDS = 100; // pages below 3200 may hold files: their bits are kept
  const FRAG_PAGES = 20; // more pieces than the 8 inline extents, which puts the extents into an index page
  const fragBig = async (knob: number) => {
    const dk = await rig.pausedBoot(await baseCard());
    await rig.runtime.ui.wait({ ms: 2000, why: SETTLE });
    for (let w = FRAG_FIRST_WORD; w < FRAG_LAST_WORD; w++) {
      const kept = w < FRAG_USED_WORDS ? await word(rig.session, BITMAP + 4 * w) : 0;
      await pokeWord(rig.session, BITMAP + 4 * w, (kept | 0x55555555) >>> 0);
    }
    await pokeKnob(rig.session, rig.plan, 'DKRT_PD_CUT', knob);
    const big = pattern(FRAG_PAGES * 32768 - 16 - 700);
    const x = await run(dk, WRITE | SAMPLES, big.length, 'Samples/Frag/big.bin');
    expect([x.close, x.job, x.devHash], 'closed with the hash of data + 16 zeros').toEqual([0, 0, slot4(big)]);
    const out = await rig.after(`pdg${knob}`);
    expect((await cat(out, 'Samples/Frag/big.bin')).subarray(0, big.length), 'bytes').toEqual(big);
    const area = inodes(out);
    const ids = fileOfSize(area, big.length + 16);
    expect(ids.length, 'one record of that size').toBe(1);
    const writes = fsWrites(out);
    await rig.runtime.ui.wait({ ms: IDLE_MS, why: SETTLE }); // the batch ends: the table is rebuilt
    return { card: out, writes, table: await hashTable(), extents: extentsOf(area, ids[0]!) };
  };
  test('a file in fragmented free space (more than 8 extents, from the cheaper reserve) is read back in runs, hashed and stored as the firmware\'s close hash does', async () => {
    const fast = await fragBig(0);
    const slow = await fragBig(0x80);
    // the file is fragmented: the firmware's own extent count in the inode, read from the card
    console.log(`fragmented file: ${fast.extents.count} extents`);
    expect(fast.extents.count, 'more extents than the 8 inline ones').toBeGreaterThan(8);
    expect(slow.extents.count, 'the same layout through the firmware close').toBe(fast.extents.count);
    expect(fast.extents.list, 'the same extents').toEqual(slow.extents.list);
    expect(fast.table, 'the RAM hash table after the rebuild').toEqual(slow.table);
    expect([...fast.writes.keys()].sort((x, y) => x - y), 'the same sectors were written').toEqual([...slow.writes.keys()].sort((x, y) => x - y));
    for (const [sector, data] of fast.writes) expect(data, `sector 0x${sector.toString(16)}`).toBe(slow.writes.get(sector));
    await rig.session.expectNoFault('fragmented close hash');
  }, 3_600_000);

  // ---- the write levers (finding 134): read-back in runs, the trailer in the last write, the cheaper reserve, new files at their name -------------
  // None has a switch. The read-back in runs is compared with the firmware's own close hash (knob 0x80): the same files, bytes, sectors written and RAM
  // hash table. The trailer in the last write has no other path to compare with, so every shape that decides where the trailer goes is checked by the
  // bytes, the size and the hash of data and 16 zero bytes, which the close compares with the host's before it names anything. The shapes use the
  // fixture's two ways of writing: WRITE (calls of 4096 bytes: the last data is split over many calls and the last call is at most 4096 bytes) and
  // BIG_CALL (one call of 64 KiB, then the rest in one call that can cover a page boundary).
  const shapes: [string, number, number, string][] = [
    // [label, kind, data size, path]
    ['two pages and a tail', WRITE, 70000, 'Samples/Kits/a.bin'],
    ['under one page: the only write is the last one', WRITE, 5000, 'Samples/b.bin'],
    ['data + 16 zeros is exactly one page', WRITE, 32752, 'Samples/c.bin'],
    ['seven pages: read-back runs of 4 and 3 pages, a partial last page', WRITE, 6 * 32768 + 12345, 'Samples/Kits/d.bin'],
    ['data + 16 zeros is four pages: one whole run', WRITE, 4 * 32768 - 16, 'Samples/e.bin'],
    ['data is a whole number of pages: the trailer is a page of its own', WRITE, 2 * 32768, 'Samples/f.bin'],
    ['data is a whole number of pages, the last call is a whole page', BIG_CALL, 3 * 32768, 'Samples/g.bin'],
    ['the last page leaves 15 bytes: the trailer crosses into the next page', WRITE, 32768 - 15, 'Samples/h.bin'],
    ['the last page leaves 1 byte: the trailer crosses into the next page', WRITE, 2 * 32768 - 1, 'Samples/i.bin'],
    ['the last page leaves 5 bytes, the last call is one whole call', BIG_CALL, 65536 + 32768 - 5, 'Samples/j.bin'],
    ['the last call covers a page boundary (whole pages and a tail of 100 bytes)', BIG_CALL, 65536 + 32768 + 100, 'Samples/k.bin'],
    ['one byte in the last page, the last call is 1 byte', WRITE, 32768 + 1, 'Samples/l.bin'],
  ];
  const uploadShapes = async (knob: number, tag: string) => {
    const dk = await rig.pausedBoot(await baseCard());
    await settle();
    await pokeKnob(rig.session, rig.plan, 'DKRT_PD_CUT', knob);
    for (const [label, kind, size, p] of shapes) {
      const d = pattern(size);
      const x = await run(dk, kind | SAMPLES, d.length, p);
      expect([x.close, x.job, x.devHash], `${label}: closed with the hash of data + 16 zeros`).toEqual([0, 0, slot4(d)]);
    }
    const card = await rig.after(tag);
    const files = await drive(card);
    for (const [label, , size, p] of shapes) {
      const d = pattern(size);
      expect(files.get(p), `${label}: size and hash`).toEqual({ size: d.length + 16, hash: slot4(d) });
      const bytes = await cat(card, p);
      expect(bytes.subarray(0, d.length), `${label}: bytes`).toEqual(d);
      expect(bytes.subarray(d.length), `${label}: the 16 zero bytes of the trailer`).toEqual(Buffer.alloc(16));
    }
    const writes = fsWrites(card);
    expect(noStagingFolder(files), 'new files are written at their name: no staging folder, no record').toBe(true);
    await rig.runtime.ui.wait({ ms: IDLE_MS, why: SETTLE });
    return { writes, table: await hashTable(), files: (await snapshot(`${tag}-idle`)).files };
  };
  test('read-back in runs, the trailer in the last write, the cheaper reserve and new files at their name end with the same bytes, hashes, sectors and hash table as the firmware close', async () => {
    const fast = await uploadShapes(0, 'pdl-fast');
    const slow = await uploadShapes(0x80, 'pdl-slow');
    expect(fast.table, 'the RAM hash table after the rebuild').toEqual(slow.table);
    expect([...fast.writes.keys()].sort((x, y) => x - y), 'the same sectors were written').toEqual([...slow.writes.keys()].sort((x, y) => x - y));
    for (const [sector, data] of fast.writes) expect(data, `sector 0x${sector.toString(16)}`).toBe(slow.writes.get(sector));
    expect(staged(fast.files), 'nothing is left in the staging folder').toEqual([]);
    await rig.session.expectNoFault('write levers');
  }, 3_600_000);

});
