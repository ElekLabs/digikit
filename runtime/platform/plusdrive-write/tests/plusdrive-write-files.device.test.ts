import { expect, test } from 'vitest';
import { pokeWord } from '@digi/regression/runtime/guest-mailbox.ts';
import { readSectors } from '@digi/regression/runtime/card-record.ts';
import { SETTLE } from '@digi/regression/runtime/panel-waits.ts';
import { pdwSuite, pattern, slot4, cat, staged, noStagingFolder, WRITE, ABORT, DELETE, BIG_CALL, REPLACE, SAMPLES, SAMPLES_ONLY, STAGING, E_EXISTS, E_FILE_IN_USE, HASH_PAGES, REC_SECTORS, BITMAP, FRAG_LAST_WORD } from './plusdrive-write-support.ts';

// +Drive writes through core/plusdrive-write, files and journal-free cases: write, replace, abort, delete, wrong hash, open files, foreign staging entries, direct write failures, full card (docs/plans/usb-fast.md 3.1, finding 120; setup: plusdrive-write-support.ts).

pdwSuite('+Drive writes through core/plusdrive-write: files and journal-free cases: write, replace, abort, delete, wrong hash, open files, foreign staging entries, direct write failures, full card', { devices: ['dt2'] }, (t) => {
  const { rig, settle, baseCard, run, snapshot, same, hashTable, inodes, at, fileOfSize } = t;
  test('a write lands at its destination with the firmware trailer and hash, creates the folders, leaves no staging file; E_EXISTS without REPLACE; REPLACE swaps; an abort leaves nothing; a delete removes the file', async () => {
    const dk = await rig.pausedBoot(await baseCard());
    await rig.runtime.ui.wait({ ms: 2000, why: SETTLE }); // the boot recovery of the staging folder
    const before = (await snapshot('pd0')).files;
    expect(before.get('Samples/old.bin')?.size, 'the seed file is there').toBeGreaterThan(0);

    const big = pattern(70000); // one 64 KiB file_write call (two 32 KiB writes) and a tail
    const a = await run(dk, BIG_CALL | SAMPLES, big.length, 'Samples/Kits/new.bin');
    expect([a.create > 0, a.close, a.job], 'created, closed, finished').toEqual([true, 0, 0]);
    expect(a.devHash, 'file_close gives the hash of the data and 16 zeros').toBe(slot4(big));
    let s = await snapshot('pd1');
    expect(s.files.get('Samples/Kits/new.bin'), 'size N+16, the hash of data + 16 zeros').toEqual({ size: big.length + 16, hash: slot4(big) });
    expect((await cat(s.card, 'Samples/Kits/new.bin')).subarray(0, big.length), 'the bytes').toEqual(big);
    expect(staged(s.files), 'the staging folder is empty').toEqual([]);
    expect(noStagingFolder(s.files), 'a new file is written at its name: no staging folder').toBe(true);
    same(before, s.files, ['Samples/Kits/new.bin', 'Samples/Kits/']);

    // no REPLACE: the existing file is E_EXISTS before any data is written
    const ex = await run(dk, WRITE | SAMPLES, 777, 'Samples/old.bin');
    expect([ex.create, ex.job]).toEqual([E_EXISTS, E_EXISTS]);
    s = await snapshot('pd2');
    expect(s.files.get('Samples/old.bin'), 'the old file is as it was').toEqual(before.get('Samples/old.bin'));
    expect(staged(s.files)).toEqual([]);

    // REPLACE
    const fresh = pattern(9000);
    const r = await run(dk, WRITE | REPLACE | SAMPLES, fresh.length, 'Samples/old.bin');
    expect([r.create > 0, r.close, r.job, r.devHash]).toEqual([true, 0, 0, slot4(fresh)]);
    s = await snapshot('pd3');
    expect(s.files.get('Samples/old.bin')).toEqual({ size: fresh.length + 16, hash: slot4(fresh) });
    expect((await cat(s.card, 'Samples/old.bin')).subarray(0, fresh.length)).toEqual(fresh);
    expect(staged(s.files)).toEqual([]);
    // a replace is staged; the folder stays for the upload session (a second replace reuses it); the runtime removes the empty folder 2 s after the last session
    expect(noStagingFolder(s.files), 'the staging folder is there right after the replace').toBe(false);
    await rig.runtime.ui.wait({ ms: 3000, why: SETTLE });
    s = await snapshot('pd3b');
    expect(noStagingFolder(s.files), 'and gone after the idle time: it exists only during an upload session').toBe(true);
    same(before, s.files, ['Samples/old.bin', 'Samples/Kits/new.bin', 'Samples/Kits/']);

    // an abort (one chunk written, then file_abort) leaves nothing, for a new path and for a replace
    const ab = await run(dk, ABORT | SAMPLES, 20000, 'Samples/aborted.bin');
    expect([ab.abort, ab.job, ab.written]).toEqual([0, 0, 4096]);
    const abr = await run(dk, ABORT | REPLACE | SAMPLES, 20000, 'Samples/old.bin');
    expect([abr.abort, abr.job]).toEqual([0, 0]);
    s = await snapshot('pd4');
    expect(s.files.has('Samples/aborted.bin'), 'no file at the aborted path').toBe(false);
    expect(s.files.get('Samples/old.bin'), 'the replaced file is untouched by the abort').toEqual({ size: fresh.length + 16, hash: slot4(fresh) });
    expect(staged(s.files)).toEqual([]);

    // a wrong expected hash is E_HASH and leaves nothing
    const bad = await run(dk, 5 | SAMPLES, 3000, 'Samples/bad.bin'); // WRITE_BAD_HASH
    expect([bad.close, bad.job]).toEqual([-56, -56]);
    s = await snapshot('pd5');
    expect(s.files.has('Samples/bad.bin')).toBe(false);
    expect(staged(s.files)).toEqual([]);

    // a folder at the destination, or a file where a folder of the path should be, is E_PATH
    const folder = await run(dk, WRITE | SAMPLES, 500, 'Samples/Sub');
    expect([folder.create, folder.job], 'a folder at the destination').toEqual([-51, -51]);
    const under = await run(dk, WRITE | SAMPLES, 500, 'keep.bin/inside.bin');
    expect([under.create, under.job], 'a file above the destination').toEqual([-51, -51]);
    const factory = await run(dk, WRITE | SAMPLES, 500, 'factory/x.bin');
    expect([factory.create, factory.job], 'inside the firmware ROM tree /factory').toEqual([-51, -51]);
    const inStaging = await run(dk, WRITE | SAMPLES, 500, `${STAGING}/s00000000`);
    expect([inStaging.create, inStaging.job], 'inside the staging folder').toEqual([-51, -51]);

    // a delete removes the file; a missing one is E_NOFILE
    const del = await run(dk, DELETE | SAMPLES_ONLY, 0, 'Samples/Kits/new.bin');
    expect([del.close, del.job]).toEqual([0, 0]);
    const gone = await run(dk, DELETE | SAMPLES_ONLY, 0, 'Samples/Kits/new.bin');
    expect(gone.job, 'E_NOFILE').toBe(-59);
    s = await snapshot('pd6');
    expect(s.files.has('Samples/Kits/new.bin')).toBe(false);
    same(before, s.files, ['Samples/old.bin', 'Samples/Kits/new.bin', 'Samples/Kits/']);
    await rig.session.expectNoFault('+Drive writes');
  }, 1_800_000);

  test('a wrong expected hash is refused and leaves the card\'s hash pages and the RAM hash table as they were', async () => {
    const dk = await rig.pausedBoot(await baseCard());
    await rig.runtime.ui.wait({ ms: 2000, why: SETTLE });
    const before = await snapshot('pdm0');
    const tableBefore = await hashTable();
    const bad = await run(dk, 5 | SAMPLES, 40000, 'Samples/bad.bin'); // WRITE_BAD_HASH: two pages and a tail
    expect([bad.close, bad.job], 'E_HASH').toEqual([-56, -56]);
    const after = await snapshot('pdm1');
    expect(after.files.has('Samples/bad.bin')).toBe(false);
    expect(staged(after.files), 'staging recovered').toEqual([]);
    same(before.files, after.files, []);
    expect(await hashTable(), 'the RAM hash table is untouched').toEqual(tableBefore);
    // the delete of the staged file may write a hash page back, so the test compares the content of the 44 hash pages, not the writes
    const pages = (card: string) => readSectors(card, HASH_PAGES[0]!, HASH_PAGES[1]! - HASH_PAGES[0]!);
    expect(pages(after.card).equals(pages(before.card)), 'the hash pages on the card hold what they held').toBe(true);
    // the inodes of the seed files and of their folders are unchanged; only the staged file's own records and the staging folder's change
    const a0 = inodes(before.card);
    const a1 = inodes(after.card);
    const seedIds = [5000, 1234, 300].flatMap((n) => [...fileOfSize(a0, n), ...fileOfSize(a0, n + 16)]); // old.bin, deep.bin, keep.bin
    expect(seedIds.length, 'the seed files were found').toBe(3);
    const root = at(a0, seedIds[2]!).readUInt32BE(8); // keep.bin's folder; the staging folder entry may touch it
    const folders = new Set(seedIds.map((id) => at(a0, id).readUInt32BE(8)));
    folders.delete(root);
    for (const id of [...seedIds, ...folders]) expect(at(a1, id).equals(at(a0, id)), `inode ${id} (a seed file or the Samples/Sub folder) is unchanged`).toBe(true);
    let changed = 0;
    for (let id = 2; id < (REC_SECTORS / 0x40) * 256; id++) if (!at(a1, id).equals(at(a0, id))) changed++;
    expect(changed, 'only the staged file and the staging folder (and the root entry for it) were touched').toBeLessThanOrEqual(4);
    // the next good file is accepted
    const ok = await run(dk, WRITE | SAMPLES, 3000, 'Samples/good.bin');
    expect([ok.close, ok.job]).toEqual([0, 0]);
    await rig.session.expectNoFault('wrong hash');
  }, 1_800_000);

  test('a destination that the stock firmware has open is E_FILE_IN_USE for a replace and a delete, and nothing changes', async () => {
    const dk = await rig.pausedBoot(await baseCard());
    await rig.runtime.ui.wait({ ms: 2000, why: SETTLE });
    const before = (await snapshot('pdo0')).files;
    // the open-file bitmap (0x46f4ded0, one bit per inode id): every id below 2048 reads as open
    for (let i = 0; i < 64; i++) await pokeWord(rig.session, 0x46f4ded0 + i * 4, 0xffffffff);
    const r = await run(dk, WRITE | REPLACE | SAMPLES, 4000, 'Samples/old.bin');
    expect([r.create, r.job], 'the replace is refused before any data is written').toEqual([E_FILE_IN_USE, E_FILE_IN_USE]);
    const d = await run(dk, DELETE | SAMPLES_ONLY, 0, 'Samples/old.bin');
    expect(d.job, 'the delete is refused').toBe(E_FILE_IN_USE);
    for (let i = 0; i < 64; i++) await pokeWord(rig.session, 0x46f4ded0 + i * 4, 0);
    const s = await snapshot('pdo1');
    same(before, s.files, []);
    expect(staged(s.files)).toEqual([]);
    const ok = await run(dk, WRITE | REPLACE | SAMPLES, 4000, 'Samples/old.bin');
    expect([ok.close, ok.job], 'closed again: the replace works').toEqual([0, 0]);
  }, 1_800_000);

  test('entries in the staging folder that are not ours are left alone and logged; the folder stays then', async () => {
    const foreign = pattern(100);
    const card = await baseCard({ [`${STAGING}/foreign.bin`]: foreign }, 'pd-foreign');
    const dk = await rig.pausedBoot(card);
    await rig.runtime.ui.wait({ ms: 3000, why: SETTLE }); // the recovery
    const before = (await snapshot('pdf0')).files;
    expect(before.has(`${STAGING}/foreign.bin`), 'recovery left it').toBe(true);
    const data = pattern(5000);
    const w = await run(dk, WRITE | REPLACE | SAMPLES, data.length, 'Samples/old.bin');
    expect([w.close, w.job]).toEqual([0, 0]);
    const s = await snapshot('pdf1');
    expect(staged(s.files), 'only the foreign file is in the folder').toEqual([`${STAGING}/foreign.bin`]);
    expect(s.files.get('Samples/old.bin')).toEqual({ size: data.length + 16, hash: slot4(data) });
    same(before, s.files, ['Samples/old.bin']);
    const next = await rig.pausedBoot(await rig.after('pdf2'));
    await rig.runtime.ui.wait({ ms: 3000, why: SETTLE });
    void next;
    const t = await snapshot('pdf3');
    expect(staged(t.files), 'a reboot does not touch it').toEqual([`${STAGING}/foreign.bin`]);
  }, 1_800_000);

  test('a failed or aborted direct write deletes its file, and a folder it created stays', async () => {
    const dk = await rig.pausedBoot(await baseCard());
    await settle();
    const before = (await snapshot('pda0')).files;
    const bad = await run(dk, 5 | SAMPLES, 3000, 'Samples/New/bad.bin'); // WRITE_BAD_HASH
    expect([bad.close, bad.job], 'E_HASH').toEqual([-56, -56]);
    const ab = await run(dk, ABORT | SAMPLES, 20000, 'Samples/New/aborted.bin');
    expect([ab.abort, ab.job]).toEqual([0, 0]);
    const s = await snapshot('pda1');
    expect(s.files.has('Samples/New/bad.bin') || s.files.has('Samples/New/aborted.bin'), 'no file is left').toBe(false);
    expect(s.files.has('Samples/New/'), 'the folder that `parents` made stays').toBe(true);
    expect(noStagingFolder(s.files)).toBe(true);
    same(before, s.files, ['Samples/New/']);
    await rig.session.expectNoFault('failed direct write');
  }, 1_800_000);

  test('a reserve that finds fewer free pages than the file needs falls back to the firmware\'s reserve: card full, nothing is left, and a smaller file still fits', async () => {
    // Four pages stay free (2880 to 2883) and every other page of the bitmap is used.
    const FREE_WORD = 90;
    const big = pattern(4 * 32768); // four pages and the trailer: more than the card holds
    const dk = await rig.pausedBoot(await baseCard());
    await settle();
    for (let w = 0; w < FRAG_LAST_WORD; w++) await pokeWord(rig.session, BITMAP + 4 * w, w === FREE_WORD ? 0xfffffff0 : 0xffffffff);
    const before = (await snapshot('pdf-s0')).files;
    const x = await run(dk, WRITE | SAMPLES, big.length, 'Samples/full.bin');
    expect([x.create, x.job], 'E_FULL from the firmware reserve').toEqual([-52, -52]);
    const s = await snapshot('pdf-s1');
    expect(s.files.has('Samples/full.bin')).toBe(false);
    expect(staged(s.files)).toEqual([]);
    same(before, s.files, []);
    // the pages the short allocation took are free again
    const small = pattern(1000);
    const ok = await run(dk, WRITE | SAMPLES, small.length, 'Samples/fits.bin');
    expect([ok.close, ok.job, ok.devHash], 'a one-page file is written afterwards').toEqual([0, 0, slot4(small)]);
    await rig.session.expectNoFault('short allocation');
  }, 1_800_000);

});
