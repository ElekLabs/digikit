import path from 'node:path';
import { readSettingsRecordIfPresent, settingsValue, type DkModule } from '@digi/dk-session';
import { expect } from 'vitest';
import { expectNoGuardHits, type PatchedSession } from '../support.ts';
import { record } from './artifacts.ts';
import { dspBytes, FREE_PC, rows } from './dsp-state.ts';
import type { FxExpectations } from './dkm-parse.ts';
import { drainInputs } from './fx-session.ts';
import { bounded, rpc, word } from './guest-access.ts';
import { GAIN, SINE } from './fixtures.ts';
import { committedFxExpectations } from './pack-needs.ts';
import { expectRuntimeUi } from './ready.ts';
import { moduleResultsNote } from './runtime-menu.ts';
import { planSymbol, validateRuntimePlan, type RuntimePlan } from './runtime-plan.ts';
import { serviceSymbol } from './service-symbols.ts';
import { pollEmulated } from './waits.ts';

export function expectGainRow(row: number[], oracle: FxExpectations): void {
  expect(row.slice(0, 12), 'exact process/descriptor/state/default words').toEqual(oracle.row);
  expect(row[12], 'adapter ran the effect').toBe(1);
  expect(row[14], 'effect is not disabled').toBe(0);
  expect(row[17], 'no effect fault').toBe(0);
}

/** Gain-fixture assertions only: fixed sine/gain module and default-emulator assumptions. */
export async function inspect(
  runtime: PatchedSession,
  plan: RuntimePlan,
  slots: number,
  wet: boolean,
  artifacts: string,
  tag: string,
  afterEpoch?: number,
) {
  const { session, dk } = runtime;
  validateRuntimePlan(plan, session);
  await rpc(session, 'run.mode', { mode: 'free' });
  await expectRuntimeUi(session, plan, artifacts, { afterEpoch });
  const names = [SINE, ...(wet ? [GAIN] : [])];
  let modules: DkModule[] = [];
  let results: number[] = [];
  try {
    await pollEmulated(
      session,
      async () => {
        modules = await bounded(() => dk.modules());
        const boot = planSymbol(plan, 'DKRT_BOOT');
        results = await Promise.all(names.map((_, i) => word(session, boot + 40 + i * 4)));
        return { names: modules.map((module) => module.name), results };
      },
      (state) => JSON.stringify(state) === JSON.stringify({ names, results: names.map(() => 0) }),
      { emulatedMs: 30_000, intervalMs: 500, message: 'the expected modules and results' },
    );
  } catch (error) {
    throw new Error(`${(error as Error).message}${await moduleResultsNote(session, plan, results)}`);
  }
  // the effect row is claimed in the commit's WRITE pass: the capacity word follows the module list, so wait for it
  const capacity = await pollEmulated(
    session,
    () => bounded(() => dk.capacity()),
    (c) => c.FX_SLOTS?.used === (wet ? 1 : 0),
    { emulatedMs: 10_000, intervalMs: 200, message: 'the FX_SLOTS in use' },
  );
  expect(capacity.FX_SLOTS).toEqual({ limit: slots, used: wet ? 1 : 0, largest: 0 });
  const settings = await bounded(() => readSettingsRecordIfPresent(dk));
  expect(settings === null ? null : settingsValue(settings, 'dk/fault')).toBeNull();
  await drainInputs(session);
  await rpc(session, 'run.mode', { mode: 'paused' });
  const boot = planSymbol(plan, 'DKRT_BOOT');
  const fields = await Promise.all(Array.from({ length: 10 + names.length }, (_, i) => word(session, boot + i * 4)));
  // runtime/core/abi.toml DkrtBootInfo: magic/version/safe/reason, count/loaded/UI, results.
  expect(fields.slice(0, 4)).toEqual([0x444b4254, 1, 0, 0]);
  expect(fields.slice(7, 10)).toEqual([names.length, names.length, 1]);
  expect(fields.slice(10)).toEqual(names.map(() => 0));
  const assigned = await serviceSymbol({ session }, plan, 'machines', 'DKRT_ASSIGN');
  expect(await word(session, assigned)).toBe(0x444b4153);
  expect(await word(session, assigned + 4)).toBe(1);
  expect(modules[0]!.types).toHaveLength(1);
  const chain = await rows(session, plan, slots);
  expect(chain.slice(wet ? 1 : 0).map((row) => row[0])).toEqual(Array(slots - (wet ? 1 : 0)).fill(FREE_PC));
  let oracle: FxExpectations | undefined;
  if (wet) {
    const effect = modules[1]!;
    expect(effect.needs, 'the effect module needs the runtime of sdk/sdk.toml').toBeGreaterThan(0);
    expect(effect.rows).toBe(0);
    const base = planSymbol(plan, '_dl_area') + effect.dsp_off;
    oracle = committedFxExpectations(session.options.device, GAIN, base); // the module that the card was built from
    record(artifacts, `${tag}-expected-gain`, oracle);
    expect(effect.dsp_len, 'MODULES range agrees with independently inferred needs').toBe(oracle.needs.DSP_AREA);
    expect(oracle.needs.FX_SLOTS).toBe(1);
    expect(capacity.FX_SLOTS!.used).toBe(oracle.needs.FX_SLOTS);
    expect(
      (await dspBytes(session, base, oracle.image_hex.length / 2)).toString('hex'),
      'independent relocated DSP image',
    ).toBe(oracle.image_hex);
    expectGainRow(chain[0]!, oracle);
  }
  const status = await bounded(() => session.status());
  expect(status.cf.main_ui_reached).toBe(true);
  expect(status.cf.input_pending).toBe(0);
  expect(status.scheduled_inputs).toBe(0);
  expect(status.dsp).toBe('sharc');
  const semantic = {
    boot: { count: fields[7], loaded: fields[8], safe: fields[2], counter: fields[4] },
    modules: modules.map(({ name, needs, types, rows: count, dsp_len }) => ({
      name,
      needs,
      types,
      rows: count,
      dsp_len,
    })),
    capacity: capacity.FX_SLOTS,
    freeRows: chain.filter((row) => row[0] === FREE_PC).length,
    settings: settings === null ? null : Buffer.from(settings).toString('hex'),
    input: { pending: status.cf.input_pending, scheduled: status.scheduled_inputs },
    dsp: status.dsp,
  };
  record(artifacts, tag, { semantic, modules, chain, status, capacity });
  return { semantic, chain, oracle, epoch: status.epoch };
}

export async function evidence(
  runtime: PatchedSession,
  artifacts: string,
  tag: string,
  overlays: string[],
  requireAudible?: boolean,
): Promise<void> {
  const { session } = runtime;
  // Capture the guard report and the overlay first, so that a fault still leaves its artifacts.
  const guard = await bounded(() => session.client.guardReport());
  const file = path.join(artifacts, `${tag}.overlay.bin`);
  const overlay = await rpc(session, 'card.overlay', { path: file });
  overlays.push(file);
  // A case in which no tap ran (for example one a test replaced by a reload before it played anything) has no PCM
  // evidence: guard, overlay and fault checks still run, unless the case requires audio. After a recorded tap start,
  // a failing report fails the case.
  const pcm = session.pcmTapStarted
    ? await rpc<{ non_finite: number; peak: number }>(session, 'pcm.tap', { action: 'report' })
    : undefined;
  record(artifacts, `${tag}-closing`, { guard, overlay, pcm, status: await bounded(() => session.status()) });
  await bounded(() => session.expectNoFault(tag));
  await bounded(() => expectNoGuardHits(session));
  if (!pcm && requireAudible) throw new Error(`${tag}: the case requires audio but no PCM tap ran`);
  if (pcm) {
    expect(pcm.non_finite).toBe(0);
    if (requireAudible) expect(pcm.peak, 'input is audible').toBeGreaterThan(0.01);
  }
}

/** The per-case evidence closer of a fixture: remembers the epoch, then checks evidence, fault, guard and PCM. */
export function evidenceCollector(
  config: { requireAudible?: RequireAudible },
  artifacts: () => string,
  overlays: string[],
  rememberEpoch: (epoch: unknown) => void,
  close: typeof evidence = evidence,
) {
  return async (runtime: PatchedSession, tag: string, requireAudible?: boolean): Promise<void> => {
    rememberEpoch((await bounded(() => runtime.session.status())).epoch);
    await close(runtime, artifacts(), tag, overlays, audibleRequired(config, runtime, requireAudible));
  };
}

/** Audio is required only when the call (`override`) or the suite sets `requireAudible`. */
export type RequireAudible = boolean | ((runtime: PatchedSession) => boolean);
export function audibleRequired(
  config: { requireAudible?: RequireAudible },
  runtime: PatchedSession,
  override?: boolean,
): boolean {
  const { requireAudible } = config;
  return override ?? (typeof requireAudible === 'function' ? requireAudible(runtime) : (requireAudible ?? false));
}
