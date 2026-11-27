import fs from 'node:fs';
import path from 'node:path';
import { moduleStateText, type ModuleState } from '@digi/dk-session';
import { RECORD_KIND } from '@digi/dk-session/gen_proto';
import { expect, test } from 'vitest';
import { cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { type CardRig } from '@digi/regression/runtime/card-rig-api.ts';
import { derivedModule } from '@digi/regression/runtime/service-files.ts';
import { moduleArtifact } from '@digi/regression/runtime/module-artifact.ts';
import { loadCode, stateName } from '@digi/regression/runtime/load-states.ts';
import { LOADED } from '@digi/regression/runtime/synth.ts';
import { bootEntries, COUPLED_ROOTS, SYMBOLS, type Entry } from '../capacity/capacity-helpers.ts';
import { edits, patched, unhashed } from './dkm-patch.ts';

/**
 * What only the firmware shows of a refusal (docs/plans/runtime-and-services.md section 8, item R11; the table of codes is `refusal-table.ts`): one boot of a card
 * with a good module, a copy of it with a wrong file hash, and a copy of `test/lfo-shape` under another name (the same shape key), read in three views that must agree:
 * `DKRT_BOOT` (`results`, `states`, `detail`, read from guest memory), the LOADS command (version 2, as Transfer reads it) and MODULES (only the ACTIVE module is listed). The
 * boot runs without the SHARC (`dsp: 'cold'`): the good module has no DSP part.
 *
 * The code each cause gets is not shown here. It is a host test, one for each code:
 * - E_SHA of a file and its state in `DKRT_BOOT`: the assembly test `a_boot_records_a_file_with_a_wrong_hash_as_failed_with_e_sha`; E_NOT_FOUND of a selected file:
 *   `manager_assembly_tests.rs` (`a_selected_file_that_is_not_on_the_card_is_reported_and_the_rest_of_the_selection_loads`); E_PLAN_KEY with the kind and the key as detail:
 *   the same file (`of_two_lfo_shapes_with_one_key_the_first_loads_and_the_second_is_refused_by_the_boot`); the state of each code: `classify` in `abi/src/state.rs`;
 * - the words of the refusal text for Transfer: `packages/dk-session/tests/loads.test.ts`;
 * - E_FORMAT (magic, sizes, a module's KINDS table), E_SHA, E_CONTRACT of the header: `loader/image.rs` (`refuses_bad_magic_format_and_sizes`, `refuses_bad_sha_and_contract`,
 *   `a_need_of_another_major_or_minor_or_a_higher_patch_is_refused`) and `loader/role.rs` (the role checks);
 * - E_MISSING and E_CONTRACT of the need table, a hard need above an API of the core: the graph (`dkrt-lifecycle` `graph/tests.rs`, `graph/from_tables/tests.rs`) and
 *   `lifecycle/codes.rs` (the code of each refusal);
 * - E_PLAN_KIND, E_PLAN_KEY, E_PLAN_FIT: `admission.rs` and the assembly tests (`the_boot_loads_accepted_modules_and_reports_a_refused_one_without_loading_it`,
 *   `the_same_machine_key_in_two_packed_modules_is_refused_by_the_boot_plan`);
 * - E_INIT: `boot.rs` and `reload.rs` (a failed init leaves nothing).
 */
const GOOD = 'test/lfo-shape';
/** The file of LFO under another module name: the same shape key. */
const LFO_DUP = 'test/lfo-shape-copy';
/** Selected after the three entries and not on the card: the real card backend must say "absent", not a read error. */
const ABSENT = 'modules/absent.dkm';

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
async function boot(rig: CardRig, tag: string, entries: Entry[], absent: string[]): Promise<Seen> {
  const got = await bootEntries(rig, tag, entries, [lfoService(rig)], absent);
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

  // one boot: entry 0 is the good module, entry 1 a copy with a wrong file hash, entry 2 a copy of the lfo module with the shape key of entry 0
  let seenBoot: Promise<Seen> | undefined;
  const seen = (): Promise<Seen> => {
    seenBoot ??= (async () => {
      await rig.first();
      const entries: Entry[] = [
        { name: GOOD, file: moduleArtifact(rig.device, GOOD), want: LOADED },
        entryOf('test/c-sha', unhashed(patched(fixture(), 'test/c-sha', withKey(101))), loadCode('E_SHA')),
        entryOf(LFO_DUP, patched(fixture(), LFO_DUP), loadCode('E_PLAN_KEY')),
      ];
      return boot(rig, 'refusal', entries, [ABSENT]);
    })();
    return seenBoot;
  };

  test('the good module is ACTIVE in DKRT_BOOT and LOADS, and MODULES lists it alone', async () => {
    const s = await seen();
    expectLoaded(s, 0);
    expect(s.modules, 'MODULES lists the good module and no refused or failed copy').toEqual([GOOD]);
  });

  test('a failed and a refused module have the same state and code in DKRT_BOOT and LOADS', async () => {
    const s = await seen();
    expectModule(s, 1, 'FAILED', loadCode('E_SHA'));
    expectModule(s, 2, 'REFUSED', loadCode('E_PLAN_KEY'));
    expect(s.detail[2]![0], 'the kind: lfo_shape').toBe(RECORD_KIND.LFO_SHAPE);
    expect(s.loads[2]!.detail, 'LOADS carries the same words as DKRT_BOOT').toEqual(s.detail[2]);
  });

  test('a selected path that is not on the card is E_NOT_FOUND / FAILED in DKRT_BOOT and LOADS, not a read error', async () => {
    const s = await seen();
    expectModule(s, 3, 'FAILED', loadCode('E_NOT_FOUND'));
    expect(s.modules, 'MODULES still lists the good module alone').toEqual([GOOD]);
  });
});
