import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { RpcData } from '@digi/dk-session/rpcdata';
import { unwrapBackup, wrapBackup } from '@digi/dk-session/backup-file';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { emulatorClock, stockImage, waitClock } from '@digi/test-support';
import { expect } from 'vitest';
import { blob, pokeWord, statusOf } from '@digi/regression/runtime/card-kit.ts';
import { word } from '@digi/regression/runtime/fx-live.ts';
import { patchkit } from '@digi/regression/runtime/patchkit.ts';
import { planSymbol, type RuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { runtimeTests } from '@digi/regression/runtime/ready.ts';

/**
 * The eMMC write guard (docs/plans/card-write-guard.md, core/card-guard) in the emulator, on the Syntakt `emulator` set (the one set of this device that the
 * emulator patches in: it holds the guard). The device file's rows are the Syntakt's: stock `[0, 0x180000)`, runtime = the store.
 *
 * - A guarded boot of a blank card runs the stock initialisation (the drive format and the working project writes), then the runtime's upload and a stock
 *   sample upload through the Data API: the guard counters stay 0 and no refusal event is logged.
 * - A refusal: the stock row is made tighter in the running image (the row table `dkcg_rows` is RAM in the emulator; the test pokes the end of row 0 to the
 *   first sector of sample slot 2), a stock sample upload into slot 2 then fails with a host error, the counters name the request, slot 2 stays empty, slot 1 and
 *   the store are unchanged, and the runtime's own write into the store still works (the runtime class is told by its return address, not by the sector).
 */
const SLOT = (n: number) => (n - 1 + 8) << 10; // the first sector of sample slot n (finding 142: `((N-1)+8)<<10`)
const ROW_END = 4; // `cg_row`: first, end, who

/** A slot image: SAMP, the length in samples at +8, a name at +0xC, 16-bit samples from +0x40. */
function image(name: string, samples: number): Uint8Array {
  const b = Buffer.alloc(0x40 + samples * 2);
  b.write('SAMP', 0);
  b.writeUInt32BE(samples, 8);
  b.write(name, 0xc);
  for (let i = 0; i < samples; i++) b.writeInt16BE(((i * 37) % 2000) - 1000, 0x40 + i * 2);
  return new Uint8Array(b);
}

/** The plan of the `emulator` set of this device: where `dkcg_rows` is in the running image. The daemon patches the same modules in the same order. */
async function planOf(device: string): Promise<RuntimePlan> {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'card-guard-plan-'));
  try {
    const planFile = path.join(dir, 'plan.json');
    await patchkit(
      ['apply', '--firmware', stockImage(device as 'st'), '--set', 'emulator', '--no-flash-cap', '-o', path.join(dir, 'image.syx'), '--plan', planFile],
      { timeout: 120_000 },
    );
    return { symbols: JSON.parse(fs.readFileSync(planFile, 'utf8')).symbols } as unknown as RuntimePlan;
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

runtimeTests(
  'card guard: a guarded boot, an upload and a stock sample write are not refused; a stock write outside its row is refused and the card keeps its bytes',
  async (runtime) => {
    const { session, dk } = runtime;
    const clock = emulatorClock(session);
    const guard = async () => {
      const g = (await dk.info()).card_guard;
      if (g === null) throw new Error('INFO reports no card guard: the image does not hold core/card-guard');
      return g;
    };
    const plan = await planOf(session.options.device);
    // the runtime's small ring keeps error events (`DKRT_LOG`: 32 events of 32 bytes: seq, time, u16 kind, u16 flags, five words); no debug module is needed to read it
    const ring = planSymbol(plan, 'DKRT_LOG');
    const refusals = async () => {
      const out: { seq: number; a: number[] }[] = [];
      for (let i = 0; i < 32; i++) {
        const at = ring + i * 32;
        const seq = await word(session, at);
        const kind = (await word(session, at + 8)) >>> 16;
        if (seq !== 0 && kind === LOG_KIND.CARD_GUARD_REFUSED)
          out.push({ seq, a: await Promise.all([0, 1, 2, 3, 4].map((k) => word(session, at + 12 + 4 * k))) });
      }
      return out;
    };
    const rows = planSymbol(plan, 'dkcg_rows');
    const rowCount = planSymbol(plan, 'dkcg_row_n');
    expect(await word(session, rowCount), 'the Syntakt has two rows: the stock window and the store').toBe(2);
    expect(await word(session, rows), 'row 0 is the stock row from sector 0').toBe(0);
    expect(await word(session, rows + ROW_END), 'the stock window ends at 0x180000').toBe(0x180000);

    // (a) a guarded boot, a runtime upload and a stock sample upload: nothing is refused
    expect(await guard(), 'after the boot (the stock drive initialisation and the working project writes ran inside the window)').toMatchObject({
      version: 1,
      refused_writes: 0,
      refused_erases: 0,
    });
    const info = await dk.info();
    expect(info.card_state, 'the card is ours').toBe(1);
    expect(await statusOf(() => dk.put('modules/s.dkm', blob(3_000, 4))), 'the runtime writes the store').toBe(0);
    expect(await guard(), 'after the runtime upload').toMatchObject({ refused_writes: 0, refused_erases: 0 });
    const data = new RpcData(dk);
    const first = image('guard one', 4_000);
    await data.writeFile(1, wrapBackup(1, first));
    expect(unwrapBackup(await data.readFile(1)).image, 'stock sample upload: slot 1 holds the image').toEqual(first);
    expect(await guard(), 'after a stock sample upload (Data API writes and the slot erase)').toMatchObject({ refused_writes: 0, refused_erases: 0 });
    await waitClock(clock, 3_000);
    expect(await refusals(), 'no refusal event').toEqual([]);
    const storeBefore = (await dk.list()).map((e) => `${e.path}:${e.size}`);

    // (b) the stock row ends where slot 2 starts: a stock sample write into slot 2 is refused, the runtime's write is not
    await pokeWord(session, rows + ROW_END, SLOT(2));
    expect(await word(session, rows + ROW_END), 'the row is tighter now').toBe(SLOT(2));
    const second = image('guard two', 4_000);
    await expect(data.writeFile(2, wrapBackup(2, second)), 'the host gets an error').rejects.toThrow();
    const g = await guard();
    expect(g, 'the slot writer erases the first sector of the slot first: that erase was the one refused request').toMatchObject({
      refused_writes: 0,
      refused_erases: 1,
      last_sector: SLOT(2),
      last_sectors: 1,
      last_class: 'stock',
      last_kind: 'erase',
    });
    expect((await data.list()).filter((r) => r.used).map((r) => r.slot), 'slot 2 stayed empty and slot 1 is still there').toEqual([1]);
    expect(unwrapBackup(await data.readFile(1)).image, 'slot 1 is unchanged').toEqual(first);
    await waitClock(clock, 3_000);
    const events = await refusals();
    expect(events.length, 'one refusal event').toBe(1);
    expect(events[0]!.a.slice(0, 2), 'the event names the refused sector and its length in sectors').toEqual([SLOT(2), expect.any(Number)]);
    expect(events[0]!.a[0]).toBe(SLOT(2));
    expect((await dk.list()).map((e) => `${e.path}:${e.size}`), 'the store is unchanged').toEqual(storeBefore);
    expect((await dk.info()).card_state, 'the card is still ours').toBe(1);
    expect(await statusOf(() => dk.put('modules/t.dkm', blob(3_000, 5))), 'the runtime still writes the store (its class is not its sector)').toBe(0);
    expect((await guard()).refused_writes + (await guard()).refused_erases, 'that write was not refused').toBe(1);
  },
  { devices: ['st'], dsp: 'cold', patchSet: 'emulator', blankCard: true },
);
