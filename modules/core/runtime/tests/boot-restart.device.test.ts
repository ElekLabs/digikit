import { expect, test } from 'vitest';
import { bootInfo, bootInfoLoaded, CMD as MAILBOX, currentRecord, pokeWord, setSelection } from '@digi/regression/runtime/card-kit.ts';
import { CARD_SYMBOLS, cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { FULL_TIER } from '@digi/regression/runtime/devices.ts';
import { pollUntil } from '@digi/test-support';
import { dynType, voiceRows } from '@digi/regression/runtime/project-kit.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';

/**
 * Select, restart, boot (the `restart` part of the Python boot proof `pk_dkrt_boot`). A card with an empty selection boots with no module.
 * The mailbox SELECT writes both record copies at seq 2. The mailbox restart (`dkrt_restart`) and the emulator's soft reset boot the same
 * card again: the module loads and claims its DSP voice rows. Selecting nothing and restarting frees the rows (full tier).
 *
 * DSP 'sharc': the voice rows live in the DSP registry, and a module's DSP part is acknowledged only by the SHARC.
 */

const MODULES = { dt2: ['test/sine'], dn2: ['test/dyn-dn2', 'test/sine'] } as const;
/** DSP voice rows: test/sine; on DN2 also test/dyn-dn2 (it carries the DSP of test/sine). */
const VOICES = { dt2: 1, dn2: 2 } as const;
const FREE_TYPE = 0xffff_ffff;
/** The stable period of the proof's records (DSP blocks): short, so the UI loop clears the boot counter within the settle time. */
const STABLE_BLOCKS = 300;
/** Failure bound, in emulated ms, of the wait for the new boot's first UI-loop pass (the wait ends when `ui_reached` is 1; the stable period of 300 DSP blocks is about 0.9 s). */
const UI_WITHIN_MS = 10_000;

cardRigs(
  'boot restart',
  {
    dsp: 'sharc',
    roots: RT,
    symbols: [...CARD_SYMBOLS, 'voices.rows', 'dyndsp_info'],
  },
  (h) => {
    test(
      'select through the mailbox, restart: the next boot loads it; select nothing, restart: it is gone',
      async () => {
        const mods = MODULES[h.device].map((id, i) => h.mod(id, `m${i}`));
        const paths = mods.map((m) => m.name);
        // The card holds the modules in its store but selects none.
        const rt = await h.boot(await h.card({ modules: mods, selection: [], stableBlocks: STABLE_BLOCKS }));
        const { session } = rt;
        await h.ready();
        const b0 = await bootInfoLoaded(session, h.plan);
        expect(b0, 'a card with an empty selection boots with no module').toMatchObject({
          count: 0,
          loaded: 0,
          safe: 0,
          seq: 1,
        });

        // The card's record is seq 1, counter 0, no selection. The boot raised the counter into copy B (seq 2, counter 1); the stable period (300 blocks
        // here) then wrote the clear into copy A (seq 3, counter 0). Wait until that has landed, then the record is known exactly.
        await pollUntil(rt.clock, () => bootInfo(session, h.plan), (info) => info.stable === 1, {
          withinMs: UI_WITHIN_MS,
          message: 'the first boot ends its stable period',
        });
        const before = await h.records();
        expect(
          before.map((r) => r && [r.seq, r.counter, r.paths.length]),
          'A holds the cleared counter (seq 3), B the raised one (seq 2)',
        ).toEqual([[3, 0, 0], [2, 1, 0]]);

        expect(await setSelection(session, h.plan, paths), 'the mailbox selection answers 0').toBe(0);
        const recs = await h.records();
        expect(recs[0], 'both copies hold the selection').not.toBeNull();
        expect(recs[0], 'the same sequence number: the copies are equal').toEqual(recs[1]);
        expect(recs[0]?.paths).toEqual(paths);
        expect([recs[0]?.seq, recs[0]?.counter], 'the selection write took seq 4 and kept the cleared counter').toEqual([4, 0]);

        /** `dkrt_restart` (mailbox command), then the emulator's soft reset; returns when the new boot has read the record's sequence number and its stable period has ended (`stable`, set in the branch that clears the boot counter). */
        const restart = async () => {
          const seq = currentRecord(await h.records())?.seq;
          await pokeWord(session, planSymbol(h.plan, 'DKRT_MAILBOX'), MAILBOX.RESTART);
          await h.waitReset();
          await pollUntil(
            rt.clock,
            () => bootInfo(session, h.plan),
            (info) => info.seq === seq && info.stable === 1,
            { withinMs: UI_WITHIN_MS, message: `the new boot (record seq ${seq}) ends its stable period` },
          );
          return bootInfoLoaded(session, h.plan);
        };

        const b1 = await restart();
        expect(b1, `the next boot loads it (results ${b1.results.join(',')})`).toMatchObject({
          loaded: mods.length,
        });
        expect(b1.seq, 'and starts from the record the selection write left').toBe(4);
        expect(b1.results, 'every module result is 0').toEqual(Array(mods.length).fill(0));
        const rows = (await voiceRows(session, h.plan)).filter(([type]) => type !== FREE_TYPE);
        expect(rows.length, `its voice rows are claimed: ${JSON.stringify(rows)}`).toBe(VOICES[h.device]);
        // the rows have the assigned types: DT2 the type of test/sine; DN2 the types of test/dyn-dn2 and test/sine
        const assigned =
          h.device === 'dt2'
            ? [await dynType(session, h.plan, 'test/sine', 'sine')]
            : [
                await dynType(session, h.plan, 'test/dyn-dn2', 'dyn_pluck'),
                await dynType(session, h.plan, 'test/sine', 'sine'),
              ];
        expect(
          [...new Set(rows.map(([type]) => type))].sort(),
          `the voice rows have the assigned types: ${JSON.stringify(rows)}`,
        ).toEqual([...assigned].sort());
        expect(currentRecord(await h.records())?.counter, 'the UI loop of the new boot cleared the counter').toBe(0);

        if (FULL_TIER) {
          expect(await setSelection(session, h.plan, []), 'select nothing').toBe(0);
          const b2 = await restart();
          expect(b2.count, 'the boot after that loads nothing').toBe(0);
          expect(
            (await voiceRows(session, h.plan)).every(([type]) => type === FREE_TYPE),
            'and the voice rows are free',
          ).toBe(true);
        }
      },
    );
  },
);
