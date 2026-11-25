import { be32, hasCap } from '@digi/dk-session';
import { expect, test } from 'vitest';
import { bootInfo, cardInfo, crashInfo } from '@digi/regression/runtime/guest-mailbox.ts';
import { currentRecord } from '@digi/regression/runtime/card-record.ts';
import { CARD_SYMBOLS, cardRigs, RT, type CardRig } from '@digi/regression/runtime/card-rig.ts';
import { crashRecord, pausedProtocol } from '@digi/regression/runtime/debug-kit.ts';
import { rpc } from '@digi/regression/runtime/guest-access.ts';

/**
 * CPU exception capture (DT2, full tier; the Python scenario crash-capture is deleted). A module raises an exception some blocks into the
 * session (an illegal instruction for the early crash, a division by zero for the late one); the firmware's default handler calls the runtime's hook, which
 * records the fault in RAM that the soft reset keeps, and resets; the next boot logs the record. EARLY: the crash is before the stable period ends (the boot
 * counter stays raised); LATE: after it (the counter was cleared, the crashed boot is counted bad at the next boot).
 *
 * Both clocks start at the same block: the stable period counts from the first UI-loop pass of the runtime (`ui_pass`, runtime/core/src/rs/src/safe_mode.rs,
 * `DKRT_UI_START`), the card's selection record shortens it to STABLE_BLOCKS (`DKRT_STABLE_BLOCKS`), and dyn-crash counts its delay from its first tick, which
 * is in the same pass (UI_FIRST and the first tick lie at the same block in the log). The test pauses at the cold boot's ready state, about 1,900 blocks after that block (from the steps the early crash takes), so the early
 * delay must be above that with room for a slower boot; the late delay lies 1,000 blocks above the period, the
 * early one 1,000 below. A wait for the crash needs about 1,015 steps of the emulator at most (WAIT_STEPS bounds it).
 */

const STABLE_BLOCKS = 5500;
const KINDS = {
  early: { blocks: 4500, mode: 0, vector: 4 }, // illegal instruction
  late: { blocks: 6500, mode: 1, vector: 5 }, // divide by zero
} as const;
/**
 * The guard of a wait for the reset after a crash, in emulator steps (4.5 DSP blocks each): the crash comes `blocks` after the first UI-loop pass and the run is paused after that
 * pass, so a wait never needs more than the longest delay in steps. It is a hang guard in emulated time; the wait itself ends on the firmware's own events (`cpu.reset`, then
 * the cold boot's `firmware.loaded` and a ready ColdFire side). Measured on the device of the test (its `devices` record): the early crash is reached in about 575 steps, the late one in about 1015.
 */
const WAIT_STEPS = Math.ceil(Math.max(...Object.values(KINDS).map((k) => k.blocks)) / 4.5);

/** What a test looks at after a boot, read while the run is paused. */
async function observe(h: CardRig) {
  const runtime = h.runtime;
  const { session } = runtime;
  const hasLog = await pausedProtocol(runtime, async (dk) => hasCap(await dk.info(), 'LOG'));
  const events = hasLog ? await pausedProtocol(runtime, (dk) => dk.readLog()) : [];
  return {
    info: await bootInfo(session, h.plan),
    crash: await crashInfo(session, h.plan),
    card: await cardInfo(session, h.plan),
    hasLog,
    rec: hasLog ? crashRecord(events) : null,
    sel: currentRecord(await h.records()),
  };
}

cardRigs(
  'crash capture',
  { dsp: 'sharc', devices: ['dt2'], full: true, faults: true, roots: RT, symbols: CARD_SYMBOLS },
  (h) => {
    for (const [kind, { blocks, mode, vector }] of Object.entries(KINDS)) {
      test(
        `${kind}: a CPU exception in a module's tick is recorded, reported by the next boot, and two in a row lead to safe mode`,
        async () => {
          const debug = h.mod('digikit/debug', 'debug'); // the log: the crash events are read over LOG, and the safe boot loads no module
          const crash = h.mod('test/dyn-crash', 'crash');
          const cfg = h.cfg('crash-after.bin', Buffer.concat([be32(blocks), Buffer.from([mode])]));
          const card = await h.card({
            modules: [debug, crash, cfg],
            selection: [debug.name, crash.name],
            stableBlocks: STABLE_BLOCKS,
          });
          const runtime = await h.boot(card, { deliverExceptions: true });
          await rpc(runtime.session, 'run.mode', { mode: 'paused' });
          const seen: Awaited<ReturnType<typeof observe>>[] = [];
          for (let boot = 0; boot < 3; boot++) {
            if (boot) await h.waitReset(WAIT_STEPS);
            seen.push(await observe(h));
            // the boot that finds the record keeps it on the card as the settings key dk/fault: store writes on purpose
            h.allowStoreWrites(await h.storeWrites());
          }
          const [b0, b1, b2] = seen as [(typeof seen)[0], (typeof seen)[0], (typeof seen)[0]];

          expect(b0.crash, 'boot 1 sees no crash record, the exception hook is installed, power-on RAM').toMatchObject({
            found: 0,
            installed: 1,
            ramKept: 0,
          });
          expect(b0.rec).toBeNull();

          // boot 2 loads the debug module again, so its log has the whole crash record; boot 3 is safe mode with no module
          const rec = b1.rec!;
          expect(rec, 'boot 2 finds the crash record of the boot before').not.toBeNull();
          expect(rec.corrupt).toBe(false);
          expect(b1.crash, 'the runtime and the log agree').toMatchObject({ found: 1, pc: rec.pc, ramKept: 1 });
          expect(rec.vector, 'the vector of the exception').toBe(vector);
          expect(rec.module?.index, 'the crash is in the second module').toBe(1);
          expect(rec.module?.name, 'the module has a name').toBeTruthy();
          expect(rec.module?.phase, 'dyn-crash crashes in its tick (phase 2)').toBe(2);
          expect(rec.regs, 'registers').toHaveLength(16);
          expect(rec.stack, 'stack words').toHaveLength(32);
          expect(rec.tail, "events before the crash from the runtime's small ring").toBeGreaterThanOrEqual(2);
          expect(rec.a7 % 4).toBe(0);

          expect(b2.crash, 'boot 3 (safe mode) found the record of the boot before: same PC').toMatchObject({
            found: 1,
            pc: b1.crash.pc,
          });
          expect(b1.crash.tail, 'tails kept').toBeGreaterThan(0);
          expect(b2.crash.tail).toBeGreaterThan(0);
          expect(b2.hasLog, 'no LOG capability without the debug module').toBe(false);

          if (kind === 'early') {
            expect(
              [b0, b1, b2].map((b) => b.info.counter),
              'boot 2 starts with counter 1 and boot 3 with 2',
            ).toEqual([0, 1, 2]);
            expect(b1.info).toMatchObject({ safe: 0, loaded: 2 });
            expect(b1.crash.stable, 'the crash came before the stable period').toBe(0);
            expect(b2.info, 'boot 3 is safe mode (counter), no module loaded').toMatchObject({
              safe: 1,
              reason: 1,
              loaded: 0,
            });
          } else {
            expect(b0.info.counter).toBe(0);
            expect(b1.info, 'the crashed boot is counted bad at the next boot').toMatchObject({
              counter: 1,
              safe: 0,
              loaded: 2,
            });
            expect(b1.crash.stable, 'the crash came after the stable period').toBe(1);
            expect(b2.info, 'two such crashes in a row make boot 3 safe mode').toMatchObject({
              counter: 2,
              safe: 1,
              reason: 1,
              loaded: 0,
            });
          }
          expect(b2.info.uiReached, 'the safe boot reaches the UI loop').toBe(1);
          expect(b2.sel?.paths, 'and keeps the selection').toEqual([debug.name, crash.name]);
        },
      );
    }
  },
);
