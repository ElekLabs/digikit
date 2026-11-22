import fs from 'node:fs';
import path from 'node:path';
import { moduleStateText, type ModuleState } from '@digi/dk-session';
import { RECORD_KIND } from '@digi/dk-session/gen_proto';
import { expect, test } from 'vitest';
import { cardRigs, type CardRig } from '@digi/regression/runtime/card-rig.ts';
import { derivedModule } from '@digi/regression/runtime/service-files.ts';
import { moduleArtifact } from '@digi/regression/runtime/install.ts';
import { loadCode, stateName } from '@digi/regression/runtime/load-states.ts';
import { LOADED } from '@digi/regression/runtime/synth.ts';
import { bootEntries, COUPLED_ROOTS, SYMBOLS, type Entry } from '../capacity/capacity-helpers.ts';
import { edits, patched, unhashed } from './dkm-patch.ts';

/**
 * What only the firmware shows of a refusal (docs/plans/runtime-and-services.md section 8, item R11; the table of codes is `refusal-table.ts`): one boot of a card
 * with a good module and copies of the packed `test/lfo-shape` with one field changed (`dkm-patch.ts`), read in three views that must agree: `DKRT_BOOT`
 * (`results`, `states`, `detail`, read from guest memory), the LOADS command (version 2, as Transfer reads it) and MODULES (only the ACTIVE module is listed). The
 * boot runs without the SHARC (`dsp: 'cold'`): the good module has no DSP part.
 *
 * One refusal for each way a module can end: FAILED by the loader (`E_SHA`), REFUSED by the rules of the core (`E_PLAN_KEY`, a second owner of a record key, which needs
 * the real lfo service file), and a selected file the card does not have (`E_NOT_FOUND`). The causes themselves are host tests, one for each code:
 * - E_FORMAT (magic, sizes, a module's KINDS table), E_SHA, E_CONTRACT of the header: `loader/image.rs` (`refuses_bad_magic_format_and_sizes`, `refuses_bad_sha_and_contract`,
 *   `a_need_of_another_major_or_minor_or_a_higher_patch_is_refused`) and `loader/role.rs` (the role checks);
 * - E_MISSING and E_CONTRACT of the need table, a hard need above an API of the core: the graph (`dkrt-lifecycle` `graph/tests.rs`, `graph/from_tables/tests.rs`) and
 *   `lifecycle/codes.rs` (the code of each refusal);
 * - E_PLAN_KIND, E_PLAN_KEY, E_PLAN_FIT: `admission.rs` and the assembly tests (`the_boot_loads_accepted_modules_and_reports_a_refused_one_without_loading_it`,
 *   `the_same_machine_key_in_two_packed_modules_is_refused_by_the_boot_plan`);
 * - E_INIT: `boot.rs` and `reload.rs` (a failed init leaves nothing).
 */
const GOOD = 'test/lfo-shape';
const LFO = 'test/lfo-shape';
/** The file of LFO under another module name: the same shape key. */
const LFO_DUP = 'test/lfo-shape-copy';

interface Seen {
  results: number[];
  states: string[];
  detail: [number, number][];
  loads: ModuleState[];
  modules: string[];
}

/** The card file of the lfo service (not selected: the loader finds it for the NEEDS of every fixture): the packed file; the card rig adds the port file it needs. */
const lfoService = (rig: CardRig) => ({ name: 'services/lfo.dks', file: moduleArtifact(rig.device, 'core/services/lfo') });

/** One boot of `entries`, and everything the tests read from it as plain values (the tests do not touch the device again). */
async function boot(rig: CardRig, tag: string, entries: Entry[]): Promise<Seen> {
  const got = await bootEntries(rig, tag, entries, [lfoService(rig)]);
  const loads = await rig.usb(got.rt, (dk) => dk.loadStates());
  return {
    results: got.info.results,
    states: got.info.states.map(stateName),
    detail: got.info.detail,
    loads,
    modules: got.mods.map((m) => m.name),
  };
}

/** `edit` with a key of its own for the fixture's record (the copies of one fixture would collide on `LFO_KEY`). */
const withKey = (key: number, edit: (f: Buffer) => void = () => {}) => (f: Buffer) => {
  edits.key(key)(f);
  edit(f);
};

/** The state, code and detail of module `i` as the boot record and the LOADS reply both say them. */
function expectModule(seen: Seen, i: number, state: string, code: number, detail?: [number, number]) {
  const at = `module ${i}`;
  expect([seen.states[i], seen.results[i]], `${at} in DKRT_BOOT`).toEqual([state, code]);
  expect([seen.loads[i]?.state, seen.loads[i]?.result, seen.loads[i]?.index], `${at} in LOADS`).toEqual([state, code, i]);
  if (detail) {
    expect(seen.detail[i], `${at}: what the plan refused on, in DKRT_BOOT`).toEqual(detail);
    expect(seen.loads[i]!.detail, `${at}: and in LOADS`).toEqual(detail);
  }
}

/** Each module loads or is ACTIVE, or DSP_LOADING while its DSP part is going (no SHARC here). */
function expectLoaded(seen: Seen, i: number) {
  expect(['ACTIVE', 'DSP_LOADING'], `module ${i} state`).toContain(seen.states[i]);
  expect([0, 1], `module ${i} result: OK or LOADING`).toContain(seen.results[i]);
  expect(seen.loads[i]!.state).toBe(seen.states[i]);
  expect(moduleStateText(seen.loads[i]!), 'a loaded module has no refusal text').toBeUndefined();
}

cardRigs('refusal fixtures and module states', { dsp: 'cold', roots: COUPLED_ROOTS, symbols: SYMBOLS }, (rig) => {
  const fixture = () => fs.readFileSync(moduleArtifact(rig.device, GOOD));
  const entryOf = (name: string, f: Buffer, want: Entry['want']): Entry => {
    const dir = path.join(rig.artifacts, 'refusal');
    fs.mkdirSync(dir, { recursive: true });
    const file = path.join(dir, `${name.split('/').at(-1)}.dkm`);
    fs.writeFileSync(file, f);
    derivedModule(moduleArtifact(rig.device, GOOD), file);
    return { name, file, want };
  };

  // one boot for all the patched copies of the good module: entry 0 is the good one
  let patchedBoot: Promise<Seen> | undefined;
  const patchedSeen = (): Promise<Seen> => {
    patchedBoot ??= (async () => {
      await rig.first();
      const entries: Entry[] = [
        { name: GOOD, file: moduleArtifact(rig.device, GOOD), want: LOADED },
        entryOf('test/c-sha', unhashed(patched(fixture(), 'test/c-sha', withKey(101))), loadCode('E_SHA')),
      ];
      return boot(rig, 'refusal-patched', entries);
    })();
    return patchedBoot;
  };

  test('the good module loads and is ACTIVE', async () => {
    const seen = await patchedSeen();
    expectLoaded(seen, 0);
    expect(seen.modules, 'MODULES lists the good module and no refused or failed copy').toEqual([GOOD]);
  });

  test('a changed sha256 is E_SHA', async () => {
    expectModule(await patchedSeen(), 1, 'FAILED', loadCode('E_SHA'));
  });

  test('a selected file the card does not have is E_NOT_FOUND', async () => {
    await rig.first();
    const rt = await rig.load('refusal-gone', {
      modules: [{ name: 'modules/m0.dkm', file: moduleArtifact(rig.device, GOOD) }, lfoService(rig)],
      selection: ['modules/m0.dkm', 'modules/gone.dkm'],
    });
    const loads = await rig.usb(rt, (dk) => dk.loadStates());
    expect(loads.map((m) => [m.state, m.result]).slice(1), 'the missing file: FAILED with E_NOT_FOUND').toEqual([['FAILED', loadCode('E_NOT_FOUND')]]);
    expect(['ACTIVE', 'DSP_LOADING']).toContain(loads[0]!.state);
  });

  let owners: Promise<Seen> | undefined;
  const ownersSeen = (): Promise<Seen> => {
    owners ??= (async () => {
      await rig.first();
      const entries: Entry[] = [
        { name: LFO, file: moduleArtifact(rig.device, LFO), want: LOADED },
        entryOf(LFO_DUP, patched(fs.readFileSync(moduleArtifact(rig.device, LFO)), LFO_DUP), loadCode('E_PLAN_KEY')),
      ];
      return boot(rig, 'refusal-owners', entries);
    })();
    return owners;
  };

  test('the first owner of the key loads', async () => {
    const seen = await ownersSeen();
    expectLoaded(seen, 0);
    expect(seen.modules).toEqual([LFO]);
  });

  test('a second owner of one key is E_PLAN_KEY, with the kind and the key', async () => {
    const seen = await ownersSeen();
    expectModule(seen, 1, 'REFUSED', loadCode('E_PLAN_KEY'));
    expect(seen.detail[1]![0], 'the kind: lfo_shape').toBe(RECORD_KIND.LFO_SHAPE);
    expect(seen.loads[1]!.detail, 'LOADS carries the same words').toEqual(seen.detail[1]);
    expect(moduleStateText(seen.loads[1]!)).toContain(`for record kind ${RECORD_KIND.LFO_SHAPE}`);
  });
});
