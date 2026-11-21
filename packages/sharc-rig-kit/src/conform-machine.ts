/** Runs the machine cases and returns their conformance report. */
import { machineCases, wordsAt, type Case } from './cases.ts';
import { describeMachine, machineBlockFailure, machineBudgetFailure } from './checks.ts';
import { SEED, seconds, type ConformOptions } from './conform-options.ts';
import { nw } from './context.ts';
import { MachineRig, type MachineRigOptions } from './machine.ts';
import { buildMachine } from './place.ts';
import { RUNNER, type CaseResult, type MachineReport } from './report.ts';
import { noise, publish, writePlanes } from './samplepool.ts';
import { budget, machineLabel, type Budget, type MachineSpec } from './spec.ts';

/** The "sample-pool" setup: the PCM planes of the case's sample (a fixed seed) and its record at pool index 0. The case's words select index 0. */
export function publishSample(rig: MachineRig, sp: MachineSpec, c: Case): void {
  const [frames, rate, channels] = c.sample!;
  const left = noise(1, frames);
  const right = channels === 1 ? left : noise(2, frames);
  const poke = (a: number, w: number) => rig.poke(a, w);
  writePlanes(poke, left, right);
  publish(poke, nw(rig.built.data[sp.sampleRecords!]!), 0, frames, rate, channels);
}

/** `RigTest`: a rig's transport and built code, made up by a host test. */
export type RigTest = Pick<MachineRigOptions, 'open' | 'built'>;

export async function runMachineCase(
  sp: MachineSpec,
  c: Case,
  bud: Budget,
  seed: number,
  test: RigTest = {},
): Promise<CaseResult> {
  const t0 = performance.now();
  const rig = await MachineRig.open(sp, { seed, recordStores: true, ...test });
  try {
    if (c.setup === 'sample-pool') publishSample(rig, sp, c);
    rig.setNote(c.note, c.vel);
    let worstSteady = 0;
    let worstNoteon = 0;
    const trig = new Set(c.trig);
    const noteonBlocks = new Set(trig);
    if (sp.device === 'dn2') for (const k of trig) noteonBlocks.add(k + 1);
    let failure: string | null = null;
    let k = 0;
    for (; k < c.blocks; k++) {
      const words = wordsAt(c, k);
      rig.setParams(words);
      if (trig.has(k)) rig.trigger();
      const b = await rig.block();
      const where = describeMachine(c.group, c.name, k, words, c.note, c.vel);
      failure = machineBlockFailure(where, b, rig.stepLimit, rig.lastCtx);
      if (failure === null) {
        const noteOn = noteonBlocks.has(k);
        if (noteOn) worstNoteon = Math.max(worstNoteon, b.instructions);
        else worstSteady = Math.max(worstSteady, b.instructions);
        failure = machineBudgetFailure(where, b.instructions, noteOn, bud);
      }
      if (failure) break;
    }
    return {
      group: c.group,
      name: c.name,
      blocks: Math.min(k, c.blocks - 1) + 1,
      steady: worstSteady,
      noteon: worstNoteon,
      failure: failure ? `${machineLabel(sp)} ${failure}` : null,
      seconds: seconds(t0),
    };
  } finally {
    await rig.close();
  }
}

export const POLY_BLOCKS = 12;
export const POLY_NOTES: [number, number][] = [
  [24, 60],
  [60, 100],
  [100, 127],
];

/**
 * Two voices on one core, notes `notes` (voice 1 starts four blocks late), all parameters at their defaults, then at their maxima from
 * block 6, then at their minima from block 9. A store of one voice into the other's state, or past either, fails.
 */
export async function runPoly(
  sp: MachineSpec,
  notes: [number, number],
  seed: number,
  test: RigTest = {},
): Promise<CaseResult> {
  const t0 = performance.now();
  const rig = await MachineRig.open(sp, { seed, recordStores: true, ...test });
  try {
    rig.select(0);
    rig.select(1);
    let failure: string | null = null;
    let k = 0;
    for (; k < POLY_BLOCKS && !failure; k++)
      for (const t of [0, 1]) {
        const level = k >= 6 && k < 9 ? 'hi' : k >= 9 ? 'lo' : null;
        rig.use(t);
        if (level) rig.setParams(sp.params.map((p) => [p.id, level === 'hi' ? p.hi : p.lo] as const));
        if (k === 4 * t) {
          rig.setNote(notes[t]!, 100);
          rig.trigger();
        }
        if (k < 4 * t) continue; // voice 1 has not started
        const b = await rig.block();
        if (!b.returned || b.foreign.length) {
          const why = b.foreign.join('; ') || 'the call did not return';
          failure = `${machineLabel(sp)} case writes/two-voices/notes${notes[0]},${notes[1]} block ${k} voice ${t}: the module wrote memory it does not own (${why})`;
          break;
        }
      }
    return {
      group: 'writes',
      name: `two-voices/notes${notes[0]},${notes[1]}`,
      blocks: k,
      steady: 0,
      noteon: 0,
      failure,
      seconds: seconds(t0),
    };
  } finally {
    await rig.close();
  }
}

/** The cases of one machine module on one device. */
export async function conformMachine(sp: MachineSpec, o: ConformOptions = {}): Promise<MachineReport> {
  const seed = o.seed ?? SEED;
  const t0 = performance.now();
  const image = buildMachine(sp); // assembles and links once; the cases reuse it
  // the broker takes `max(costs) x voices` of the selection, so a machine that declares fewer voices than the device's 16 may use its share of the headroom
  const all = budget(sp.device);
  const voices = image.desc[3] ?? 0;
  const bud = voices >= 1 && voices < 16 ? { ...all, voiceBlock: Math.floor(all.headroom / voices) } : all;
  const built = (performance.now() - t0) / 1000;
  const groups = o.groups ?? [];
  const cases: CaseResult[] = [];
  for (const c of machineCases(sp))
    if (!groups.length || groups.includes(c.group))
      cases.push(await runMachineCase(sp, c, bud, seed, { open: o.open }));
  if (!groups.length || groups.includes('writes'))
    for (const notes of POLY_NOTES) cases.push(await runPoly(sp, notes, seed, { open: o.open }));
  return {
    label: machineLabel(sp),
    runner: RUNNER,
    build_seconds: Math.round(built * 10) / 10,
    budget: { voice_block: bud.voiceBlock, note_on_block: bud.noteOnBlock, headroom: bud.headroom },
    params: sp.params.map((p) => p.id),
    cases,
  };
}
