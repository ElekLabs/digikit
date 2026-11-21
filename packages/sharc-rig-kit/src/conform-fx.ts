/** Runs the effect cases and returns their conformance report. */
import { fxCases, fxWordsAt, type FxCase } from './cases.ts';
import { describeFx, fxBlockFailure, fxBudgetFailure, signalBlock } from './checks.ts';
import { SEED, seconds, type ConformOptions } from './conform-options.ts';
import { P0, FxRig, type FxRigOptions } from './fx.ts';
import { buildFx } from './place.ts';
import { PyRandom } from './prng.ts';
import { RUNNER, type CaseResult, type FxReport } from './report.ts';
import { FX_ADAPTER_MAX, fxBudget, fxLabel, fxWord, type FxSpec } from './spec.ts';

export type FxRigTest = Pick<FxRigOptions, 'open' | 'built'>;

export async function runFxCase(
  sp: FxSpec,
  c: FxCase,
  effectBudget: number,
  desc: number[],
  seed: number,
  test: FxRigTest = {},
): Promise<CaseResult> {
  const t0 = performance.now();
  const rig = await FxRig.open([{ pc: sp.process, state: sp.state, desc: sp.desc }], {
    sources: sp.sources,
    seed,
    recordStores: true,
    ...test,
  });
  try {
    const rng = new PyRandom(seed);
    const stack = desc[5]!;
    let worst = 0;
    let failure: string | null = null;
    let on = true;
    let faults = 0; // the row's fault count before the block (a fresh table holds 0)
    let k = 0;
    for (; k < c.blocks; k++) {
      const raw = fxWordsAt(c, k);
      const words = raw.map((w, j) => fxWord(sp, j, w));
      words.forEach((w, j) => rig.setRowWord(0, P0 + j, w));
      if (c.toggle && k && k % c.toggle === 0) {
        on = !on;
        rig.setEnabled(0, on);
      }
      const b = await rig.block(signalBlock(c.signal, k, rng));
      const where = describeFx(c.group, c.name, k, c.signal, words);
      failure = fxBlockFailure(where, b, rig.stepLimit, faults, () => rig.foreignStores(stack, b.stores));
      faults = b.faults[0]!;
      if (failure === null) {
        worst = Math.max(worst, b.instructions);
        failure = fxBudgetFailure(where, b.instructions, effectBudget);
      }
      if (failure) break;
    }
    return {
      group: c.group,
      name: c.name,
      blocks: Math.min(k, c.blocks - 1) + 1,
      steady: worst,
      noteon: 0,
      failure: failure ? `${fxLabel(sp)} ${failure}` : null,
      seconds: seconds(t0),
    };
  } finally {
    await rig.close();
  }
}

/** The problems of an effect's description (`dk_fx_desc` words): magic, contract, parameter count, state size. */
export function descriptionProblems(desc: readonly number[]): string[] {
  const problems: string[] = [];
  if (desc[0] !== 0x444b4658) problems.push('the description has no DK_FX_MAGIC');
  if (desc[1] !== 1) problems.push(`contract ${desc[1]} is not DK_FX_V1`);
  if (desc[2]! > 8) problems.push(`${desc[2]} parameters, at most 8`);
  if (desc[3]! % 4) problems.push(`state_bytes ${desc[3]} is not a multiple of 4`);
  return problems;
}

/** The cases of one effect module. */
export async function conformFx(sp: FxSpec, o: ConformOptions = {}): Promise<FxReport> {
  const t0 = performance.now();
  const built = buildFx(sp.sources);
  const desc = descriptionOf(built, sp.desc);
  const buildSeconds = (performance.now() - t0) / 1000;
  const problems = descriptionProblems(desc);
  const effectBudget = fxBudget();
  const seed = o.seed ?? SEED;
  const groups = o.groups ?? [];
  const cases: CaseResult[] = [];
  if (problems.length)
    cases.push({
      group: 'signals',
      name: 'description',
      blocks: 0,
      steady: 0,
      noteon: 0,
      failure: `${fxLabel(sp)} ${problems.join('; ')}`,
      seconds: 0,
    });
  else
    for (const c of fxCases(desc[2]!))
      if (!groups.length || groups.includes(c.group))
        cases.push(await runFxCase(sp, c, effectBudget, desc, seed, { open: o.open }));
  return {
    label: fxLabel(sp),
    runner: RUNNER,
    build_seconds: Math.round(buildSeconds * 10) / 10,
    budget: { effect: effectBudget, adapter: FX_ADAPTER_MAX },
    n_params: desc[2]!,
    cases,
  };
}

function descriptionOf(built: ReturnType<typeof buildFx>, symbol: string, n = 6): number[] {
  const at = built.data[symbol];
  for (const init of built.inits)
    if (at !== undefined && init.address <= at && at < init.address + init.bytes.length) {
      const view = new DataView(init.bytes.buffer, init.bytes.byteOffset, init.bytes.byteLength);
      return Array.from({ length: n }, (_, k) => view.getUint32(at - init.address + 4 * k, true));
    }
  throw new Error(`no data symbol ${symbol}`);
}
