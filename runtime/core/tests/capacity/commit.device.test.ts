import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { RESOURCE } from '@digi/dk-session/gen_proto';
import { committedRecords } from '@digi/test-support/commit-records';
import { expect, test } from 'vitest';
import { cardRigs, type CardRig } from '@digi/regression/runtime/card-rig.ts';
import { dspBytes, word } from '@digi/regression/runtime/fx-live.ts';
import { derivedModule } from '@digi/regression/runtime/service-files.ts';
import { moduleArtifact } from '@digi/regression/runtime/install.ts';
import { patchkit } from '@digi/regression/runtime/patchkit.ts';
import { dynType } from '@digi/regression/runtime/project-kit.ts';
import { moduleRingEvents } from '@digi/regression/runtime/runtime-menu.ts';
import { packageSymbol, planSymbol } from '@digi/regression/runtime/runtime-plan.ts';
import { LOADED } from '@digi/regression/runtime/synth.ts';
import { bootEntries, COUPLED_ROOTS, SYMBOLS, type Entry } from './capacity-helpers.ts';

/**
 * The commit of a module (docs/plans/sdk.md 6.3), end to end on both devices, with the fixture test/commit-demo: a machine `cmach` with three
 * params (Mode is latched, Go has a click action) and an effect `cfx` with one page, declared by builder calls in `init`.
 *
 * - the MODULE_COMMIT event gives the rows the module uses and the committed record block; the block holds the params and the effect page;
 * - the machine's voice row in the DSP has the knob ranges and the latch mask of the params;
 * - a module that declares more rows than its `[uses]` is refused with E_DECL_USES and changes nothing on the real tables: no row, no DSP range, no arena block, no service left
 *   up for it. The other refusals are host tests: E_DECL_ID (an id that is not in the keys table: `services/machines/tests/build.rs`, `services/fx/tests/build.rs`,
 *   `collect/tests.rs`), E_DECL_USES itself (`services/machines/tests/build.rs`), E_PLAN_KEY (an effect id that another module has: `admission.rs`) and that a refused
 *   consumer takes down the service this boot started for it (`dkrt-lifecycle` `manager/tests/unwind.rs`, `bring_up.rs`).
 *
 * The services the new boot starts only for a module are taken down again when that module is refused (the manager's unwind), so the arena of a refused boot holds no provider
 * block; the MOD_LOAD events of a provider that came up are still logged (the boot report lists what came up, then what was refused), which is why the test no longer counts
 * provider events to know the arena it expects.
 */
const ID = 'test/commit-demo';
const E_DECL_USES = -28;
/** Header fields of a .dkm (docs/design/digikit-dkm.md). */
const H_KEYS_OFF = 0x90;
const ROW_BYTES = 96;
const ROW_RANGE0 = 2;
const ROW_LATCH = 22;
/** The offset of the count of resource `id` in the USES entry (tag 2) of the header table (packages/dkneeds/src/keys.rs). */
function usesCountAt(f: Buffer, id: number): number {
  const table = f.readUInt32BE(H_KEYS_OFF);
  let at = table + 8;
  for (let i = f.readUInt16BE(table + 6); i > 0; i--) {
    const count = f.readUInt16BE(at + 2);
    if (f.readUInt16BE(at) === 2) {
      for (let k = 0; k < count; k++) if (f.readUInt32BE(at + 8 + 8 * k) === id) return at + 8 + 8 * k + 4;
    }
    at += 8 + f.readUInt32BE(at + 4);
  }
  throw new Error(`no uses entry for resource ${id}`);
}

/** Knob position of each param (`[adapter.machine] param_slots` of the device file), the sound-array slot of knob 0 and the stock row of the default glyph. */
const DEVICE = {
  dt2: { positions: [1, 2, 4], slotBase: 0, encoder: 0xcc, pageRows: 0 },
  dn2: { positions: [1, 2, 4], slotBase: 25, encoder: 0xd5, pageRows: 1 },
} as const;

/** The file with its sha256 made right again, so that only the patched field is wrong. */
function rehash(file: Buffer): Buffer {
  file.fill(0, 0x10, 0x30);
  createHash('sha256').update(file).digest().copy(file, 0x10);
  return file;
}

/**
 * Boot the fixture alone: the good boot of a rig (the first test reads its commit event and records).
 */
async function bootGood(rig: CardRig): Promise<Awaited<ReturnType<typeof bootEntries>>> {
  const entries: Entry[] = [{ name: ID, file: moduleArtifact(rig.device, ID), want: LOADED }];
  return bootEntries(rig, 'commit-good', entries);
}

/**
 * Refused and left nothing behind: no module listed, no row, no DSP range, no arena block, no MODULE_COMMIT event. With the only module refused, the new boot has taken down
 * the services it started for it, so the arena is empty (before RC2b the providers' blocks stayed: a provider logged as loaded is no longer one that is still in the arena).
 */
async function expectNothing(rig: CardRig, got: Awaited<ReturnType<typeof bootEntries>>) {
  const { mods, cap, rt } = got;
  expect(mods.map((m) => m.name), 'MODULES lists no module').toEqual([]);
  expect(
    [cap.MACHINE_ROWS!.used, cap.PARAM_ROWS!.used, cap.VOICE_ROWS!.used],
    'the refused module owns no machine, parameter or voice row (its number may hold a MISSING placeholder, which CAPACITY does not count)',
  ).toEqual([0, 0, 0]);
  expect(cap.DSP_AREA!.used, 'and holds no DSP range').toBe(0);
  expect(cap.ARENA!.used, 'and no arena block: the services started for it went down with it').toBe(0);
  expect((await word(rt.session, await packageSymbol(rt.session, rig.plan, 'TABLE_BAD'))) >>> 24, 'the firmware tables are intact').toBe(0);
  expect(await moduleRingEvents(rt.session, rig.plan, 'MODULE_COMMIT'), 'a refused commit logs no MODULE_COMMIT').toHaveLength(0);
}

cardRigs(
  'commit of a module',
  {
    dsp: 'sharc',
    roots: COUPLED_ROOTS,
    hold: true,
    symbols: [...SYMBOLS, 'voices.rows', 'dyndsp_info'],
  },
  (rig) => {
    const fixture = () => fs.readFileSync(moduleArtifact(rig.device, ID));
    const patched = (name: string, edit: (f: Buffer) => void): string => {
      const f = fixture();
      edit(f);
      const file = path.join(rig.artifacts, name);
      fs.mkdirSync(rig.artifacts, { recursive: true });
      fs.writeFileSync(file, rehash(f));
      return derivedModule(moduleArtifact(rig.device, ID), file);
    };

    test('the commit logs its rows, writes the voice row ranges and latch mask and commits the effect page', async () => {
      await rig.first();
      const dev = DEVICE[rig.device as keyof typeof DEVICE];
      const got = await bootGood(rig);
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

      // the committed records: three machine params on the device's knob positions with the stock row of the default glyph, and the effect page
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

    test('more rows than [uses] is E_DECL_USES and writes nothing', async () => {
      await rig.first();
      const file = patched('uses.dkm', (f) => {
        const at = usesCountAt(f, RESOURCE.PARAM_ROWS);
        expect(f.readUInt32BE(at), 'the fixture declares three param rows').toBe(3);
        f.writeUInt32BE(2, at);
      });
      const got = await bootEntries(rig, 'commit-uses', [{ name: ID, file, want: E_DECL_USES }]);
      expect(got.info.results).toEqual([E_DECL_USES]);
      await expectNothing(rig, got);
    });
  },
);
