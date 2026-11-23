import { committedRecords } from '@digi/test-support/commit-records';
import { expect, test } from 'vitest';
import { cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { dspBytes, word } from '@digi/regression/runtime/fx-live.ts';
import { moduleArtifact } from '@digi/regression/runtime/install.ts';
import { patchkit } from '@digi/regression/runtime/patchkit.ts';
import { dynType } from '@digi/regression/runtime/project-kit.ts';
import { moduleRingEvents } from '@digi/regression/runtime/runtime-menu.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';
import { LOADED } from '@digi/regression/runtime/synth.ts';
import { bootEntries, COUPLED_ROOTS, SYMBOLS, type Entry } from './capacity-helpers.ts';

/**
 * The commit of a module (docs/plans/sdk.md 6.3) on both devices, with the fixture test/commit-demo: a machine `cmach` with three params (Mode is latched) and an effect `cfx` with
 * one page. Only what the firmware shows stays here:
 *
 * - the MODULE_COMMIT event and the committed record block in firmware memory agree on the rows the module uses, and the block holds the params and the effect page as firmware
 *   memory has them (the host golden runs `lower()` on a hand-built Build, not on the packed fixture's init and commit);
 * - the machine's voice row in the real DSP memory has the knob ranges and the latch mask of the params.
 *
 * Owned below the device: the lowering of a hand-built Build (params with their slots, templates and ranges, the effect page) is the golden commit-demo cases of both devices
 * (`services/machines/tests/build.rs` and `services/fx/tests/build.rs` against `tests/fixtures/records/lowered.golden`); the commit refusals are host tests (E_DECL_ID: `services/machines/tests/build.rs`,
 * `services/fx/tests/build.rs`, `collect/tests.rs`; E_DECL_USES: `services/machines/tests/build.rs`, and through the real loader with nothing left behind
 * `a_machine_declared_beyond_its_uses_is_refused_with_e_decl_uses_and_leaves_nothing` in `loader/tests/assembly_tests.rs`; E_PLAN_KEY: `admission.rs`; the unwind of a refused
 * consumer: `dkrt-lifecycle` `manager/tests/unwind.rs`, `bring_up.rs`).
 */
const ID = 'test/commit-demo';
/** The voice row of the DSP (96 bytes): the knob ranges start at word 2 and the latch mask is word 22. */
const ROW_BYTES = 96;
const ROW_RANGE0 = 2;
const ROW_LATCH = 22;
/** Knob position of each param (`[adapter.machine] param_slots` of the device file), the sound-array slot of knob 0, the stock row of the default glyph and the page rows the device uses. */
const DEVICE = {
  dt2: { positions: [1, 2, 4], slotBase: 0, encoder: 0xcc, pageRows: 0 },
  dn2: { positions: [1, 2, 4], slotBase: 25, encoder: 0xd5, pageRows: 1 },
} as const;

cardRigs(
  'commit of a module',
  {
    dsp: 'sharc',
    roots: COUPLED_ROOTS,
    hold: true,
    symbols: [...SYMBOLS, 'voices.rows', 'dyndsp_info'],
  },
  (rig) => {
    test('the commit event and record block agree on the rows, and the voice row holds the knob ranges and latch mask', async () => {
      await rig.first();
      const dev = DEVICE[rig.device as keyof typeof DEVICE];
      const entries: Entry[] = [{ name: ID, file: moduleArtifact(rig.device, ID), want: LOADED }];
      const got = await bootEntries(rig, 'commit-good', entries);
      expect(got.mods.map((m) => m.name)).toEqual([ID]);
      const { session } = got.rt;

      const [event, ...more] = await moduleRingEvents(session, rig.plan, 'MODULE_COMMIT');
      expect(event, 'a MODULE_COMMIT event').toBeDefined();
      expect(more, 'one commit').toEqual([]);
      const { commit, records } = await committedRecords(event!, (a) => session.client.memPeek(a), patchkit);
      expect(commit.index, 'the selection index of the module').toBe(0);
      expect([commit.machineRows, commit.paramRows, commit.pageRows], 'the rows the declarations use').toEqual([
        1,
        3,
        dev.pageRows,
      ]);

      // the committed records in firmware memory: three machine params on the device's knob positions with the stock row of the default glyph, and the effect page
      expect(
        records.params.map((p) => [p.name, p.short, p.slot, p.template, p.min, p.max, p.default]),
        'the params of the machine',
      ).toEqual([
        ['Tone', 'TONE', dev.slotBase + dev.positions[0], dev.encoder, 0, 127 * 256, 64 * 256],
        ['Mode', 'MODE', dev.slotBase + dev.positions[1], dev.encoder, 0, 3 * 256, 0],
        ['Go', 'GO', dev.slotBase + dev.positions[2], dev.encoder, 0, 127 * 256, 0],
      ]);
      expect(records.effects, 'one effect page').toHaveLength(1);
      const page = records.effects[0] as { tab: string; knobs: Record<string, unknown>[] };
      expect(page.tab, 'the short name is the tab label').toBe('CFX');
      expect(page.knobs, 'one knob: Gain 0 to 1.27, default 0.5').toHaveLength(1);
      expect(page.knobs[0]).toMatchObject({ name: 'Gain', short: 'GAIN', min: 0, default: 0.5 });
      expect((page.knobs[0] as { max: number }).max).toBeCloseTo(1.27, 5);

      // the voice row of the machine: the knob ranges of its params and the latch mask (bit 1: the word of Mode)
      const rows = planSymbol(rig.plan, 'voices.rows');
      const first = await word(session, planSymbol(rig.plan, 'dyndsp_info') + 4 * 6);
      const type = await dynType(session, rig.plan, ID, 'cmach');
      let row: number[] | undefined;
      for (let k = 0; k < 8 && !row; k++) {
        const raw = await dspBytes(session, rows + ROW_BYTES * (first + k), ROW_BYTES);
        const words = Array.from({ length: ROW_BYTES / 4 }, (_, i) => raw.readUInt32LE(4 * i));
        if (words[0] === type) row = words;
      }
      expect(row, `a voice row of type ${type}`).toBeDefined();
      const ranges = Array.from({ length: 8 }, (_, j) => row![ROW_RANGE0 + j]);
      const want = Array(8).fill(0);
      want[dev.positions[0]] = (127 * 256 * 65536) >>> 0;
      want[dev.positions[1]] = (3 * 256 * 65536) >>> 0;
      want[dev.positions[2]] = (127 * 256 * 65536) >>> 0;
      expect(ranges, 'min | max << 16 of the param on each knob, 0 on the others').toEqual(want);
      expect(row![ROW_LATCH], 'the latch mask: the word of Mode').toBe(0b10);
      await session.expectNoFault('commit');
    });
  },
);
