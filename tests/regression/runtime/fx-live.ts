import { execFile } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import { readSettingsRecordIfPresent, settingsValue, type DkModule } from '@digi/dk-session';
import {
  button,
  MS,
  repoRoot,
  type Session,
  withinRuntimeDeadline,
  type Device,
  type RuntimeBaseline,
} from '@digi/test-support';
import { expect } from 'vitest';
import { expectNoGuardHits, withReusablePatchedSession, type PatchedSession } from '../support.ts';
import { FIRMWARE, expectRuntimeUi, moduleArtifact, record } from './live.ts';
import { planSymbol, prepareRuntimePlanOffline, validateRuntimePlan, type RuntimePlan } from './runtime-plan.ts';

const exec = promisify(execFile);
const root = repoRoot();
// core/fx's published registry row: modules/core/adapters/fx/module.toml.
export const ROW_WORDS = 24;
export const FREE_PC = 0xffffffff;
export const SINE = 'test/sine';
export const GAIN = 'test/dyn-fx-gain';
export const bounded = <T>(operation: () => Promise<T>) => withinRuntimeDeadline(Date.now() + 30_000, operation);
export const rpc = <T>(session: Session, method: string, params?: Record<string, unknown>) =>
  bounded(() => session.client.call<T>(method, params));

export function options(device: Device) {
  const binary = process.env[device === 'dt2' ? 'DIGI_EMUD_DT2' : 'DIGI_EMUD_DN2'];
  if (!binary) throw new Error(`DIGI_EMUD_${device.toUpperCase()} must name a preflighted daemon`);
  return { device, firmware: FIRMWARE[device], binary, autoNo: true };
}

export function slotsFor(device: Device): number {
  const file = path.join(root, 'devices', device === 'dt2' ? 'digitakt-ii.toml' : 'digitone-ii.toml');
  const slots = /^fx_slots\s*=\s*(\d+)/m.exec(fs.readFileSync(file, 'utf8'))?.[1];
  if (!slots) throw new Error(`missing fx_slots in ${file}`);
  return Number(slots);
}

/** Native plusdrive owns all card/store/selection encoding. Each image has its own overlay-free directory. */
async function baseline(
  session: Session,
  plan: RuntimePlan,
  wet: boolean,
  artifacts: string,
): Promise<RuntimeBaseline> {
  const tag = wet ? 'wet' : 'dry';
  const directory = path.join(session.runDir, tag);
  const seed = path.join(directory, 'seed');
  const modules = path.join(seed, 'digikit', 'modules');
  fs.mkdirSync(modules, { recursive: true });
  for (const id of [SINE, GAIN]) {
    let artifact: string;
    try {
      artifact = moduleArtifact(session.options.device, id);
    } catch (error) {
      throw new Error(`FX reuse needs prepared ${id} and the prebuilt native plusdrive card builder; ${String(error)}`);
    }
    fs.copyFileSync(artifact, path.join(modules, `${id.split('/')[1]}.dkm`));
  }
  fs.writeFileSync(
    path.join(seed, 'digikit', 'selection.txt'),
    ['modules/sine.dkm', ...(wet ? ['modules/dyn-fx-gain.dkm'] : [])].join('\n') + '\n',
  );
  const binary = path.join(root, 'out/native/target/nogen/release/plusdrive');
  if (!fs.existsSync(binary))
    throw new Error(
      `prepare prebuilt plusdrive separately: ${binary}; this fixture also needs prepared sine and gain DKMs`,
    );
  const cardImage = path.join(directory, 'card.img');
  const built = await exec(binary, ['build', seed, '-o', cardImage, '--device', session.options.device], {
    cwd: root,
    timeout: 30_000,
  });
  record(artifacts, `card-${tag}-${path.basename(session.runDir)}`, { stdout: built.stdout, stderr: built.stderr });
  const boot = planSymbol(plan, 'DKRT_BOOT');
  return {
    cardImage,
    // A targeted run-time invariant guard, not the legacy launch-time expansion write-watch/flush proof.
    guard: { protect: [{ lo: boot, hi: boot + 8, label: 'runtime boot identity' }], allow_pc: [] },
  };
}

export async function analysisJson<T>(module: string, args: string[], artifacts: string, tag: string): Promise<T> {
  try {
    const result = await exec('uv', ['run', '--no-sync', 'python', '-m', module, ...args], {
      cwd: root,
      timeout: 30_000,
      maxBuffer: 4 * 1024 * 1024,
      env: { ...process.env, UV_NO_SYNC: '1' },
    });
    const value = JSON.parse(result.stdout) as T;
    record(artifacts, tag, value);
    return value;
  } catch (error) {
    const output = error as { stdout?: string; stderr?: string };
    record(artifacts, tag, { error: String(error), stdout: output.stdout, stderr: output.stderr });
    throw error;
  }
}

interface FxExpectations {
  image_hex: string;
  row: number[];
  needs: Record<string, number>;
}
export function expectGainRow(row: number[], oracle: FxExpectations): void {
  expect(row.slice(0, 12), 'exact process/descriptor/state/default words').toEqual(oracle.row);
  expect(row[12], 'adapter ran the effect').toBe(1);
  expect(row[14], 'effect is not disabled').toBe(0);
  expect(row[17], 'no effect fault').toBe(0);
}

export async function word(session: Session, address: number): Promise<number> {
  const value = await bounded(() => session.client.memPeek(address));
  if (value === null) throw new Error(`unmapped CF word at ${address.toString(16)}`);
  return value >>> 0;
}

export async function dspBytes(session: Session, address: number, length: number): Promise<Buffer> {
  const result = await rpc<{ hex: string; unknown: number }>(session, 'dsp.peek', { addr: address, len: length });
  expect(result.unknown, 'DSP bytes are mapped').toBe(0);
  const bytes = Buffer.from(result.hex, 'hex');
  expect(bytes.length).toBe(length);
  return bytes;
}

export async function dspWords(session: Session, address: number, count: number): Promise<number[]> {
  const bytes = await dspBytes(session, address, count * 4);
  return Array.from({ length: count }, (_, i) => bytes.readUInt32LE(i * 4));
}

export async function rows(session: Session, plan: RuntimePlan, slots: number): Promise<number[][]> {
  const words = await dspWords(session, planSymbol(plan, 'fx.rows'), ROW_WORDS * slots);
  return Array.from({ length: slots }, (_, i) => words.slice(i * ROW_WORDS, (i + 1) * ROW_WORDS));
}

export async function frames(session: Session): Promise<number> {
  const frame = (await bounded(() => session.status())).dsp_frames;
  if (typeof frame !== 'number') throw new Error('status lacks DSP frame count');
  return frame;
}

export async function advance(session: Session, ms: number): Promise<void> {
  await bounded(() => session.client.step(Math.floor(ms * MS), { timeout_wall_s: 30 }));
}

/** Choose the original device list hint or a numeric override without asserting selected-track identity.
 * The BASIC chain uses selectSine for DT2 type readback; gain/deselect retains its audible-input contract. */
export async function pickSine(session: Session, selection?: number): Promise<void> {
  const tap = async (key: string, held?: string) => {
    try {
      if (held) {
        await bounded(() => session.client.inputButton(button(held), true));
        await advance(session, 5);
      }
      await bounded(() => session.client.inputTap(button(key), 20 * MS));
      await advance(session, 25);
    } finally {
      if (held) {
        await bounded(() => session.client.inputButton(button(held), false));
        await advance(session, 5);
      }
    }
  };
  const hint = selection ?? (session.options.device === 'dt2' ? 7 : 5);
  if (!Number.isInteger(hint) || hint < 0) throw new Error('FX input hint must be a nonnegative list position');
  await tap('SRC', 'FUNC');
  await advance(session, 200);
  for (let i = 0; i < hint; i++) {
    await tap('DOWN');
    await advance(session, 80);
  }
  await tap('YES');
  await advance(session, 300);
  await tap('YES');
  await advance(session, 300);
}

/** Future-frame choice follows the legacy restarted-note path; boundary offset and 1500-frame hold are unchanged. */
export async function capture(
  runtime: PatchedSession,
  file: string,
  selection?: number,
  triggerFrame?: number,
): Promise<number> {
  const { session } = runtime;
  await rpc(session, 'run.mode', { mode: 'paused' });
  await rpc(session, 'pcm.tap', { action: 'start' });
  await pickSine(session, selection);
  const current = await frames(session);
  const trig = triggerFrame ?? current + 300;
  record(path.dirname(file), `${path.basename(file, path.extname(file))}-timing`, {
    currentFrame: current,
    triggerFrame: trig,
  });
  try {
    if (current >= trig - 20) throw new Error(`FX capture setup too late: current frame ${current}, trigger ${trig}`);
    await bounded(() => session.client.runUntil({ frames: trig, timeout_wall_s: 30 }));
    await bounded(() => session.client.step(44_000, { timeout_wall_s: 30 }));
    await rpc(session, 'pcm.tap', { action: 'capture', path: file });
    await bounded(() => session.client.inputButton(button('TRIG 1'), true));
    await bounded(() => session.client.runUntil({ frames: trig + 1500, timeout_wall_s: 30 }));
  } finally {
    try {
      await rpc(session, 'pcm.tap', { action: 'capture_stop' });
    } finally {
      await bounded(() => session.client.inputButton(button('TRIG 1'), false));
    }
  }
  await advance(session, 50);
  await bounded(() => session.expectNoFault('FX note'));
  return trig;
}

interface Metrics {
  identical: boolean;
  max_err_ulps: number;
  rms_db: number | null;
  rms_base: number;
  shift: number | null;
}
export async function compare(
  base: string,
  sample: string,
  gain: number,
  artifacts: string,
  tag: string,
  identical = false,
): Promise<void> {
  let result: { stdout: string; stderr: string };
  try {
    result = await exec(
      'uv',
      [
        'run',
        '--no-sync',
        'python',
        '-m',
        'tools.patchkit.hook_ab',
        'compare',
        base,
        sample,
        '--gain',
        String(gain),
        '--align-onset',
        '--json',
      ],
      { cwd: root, timeout: 30_000 },
    );
  } catch (error) {
    const output = error as { stdout?: string; stderr?: string };
    record(artifacts, `pcm-${tag}`, { error: String(error), stdout: output.stdout, stderr: output.stderr });
    throw error;
  }
  const metrics = JSON.parse(result.stdout) as Metrics;
  record(artifacts, `pcm-${tag}`, metrics);
  expect(metrics.rms_base, 'reference has sound').toBeGreaterThan(0);
  expect(metrics.max_err_ulps).toBeLessThanOrEqual(2);
  expect(metrics.rms_db).not.toBeNull();
  expect(Math.abs(metrics.rms_db! - 20 * Math.log10(gain))).toBeLessThan(0.001);
  expect(metrics.shift).not.toBeNull();
  expect(Math.abs(metrics.shift!)).toBeLessThanOrEqual(128);
  if (identical) expect(metrics.identical, 'dry PCM is bit-identical from onset').toBe(true);
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
  const deadline = Date.now() + 30_000;
  let modules: DkModule[] = [];
  await expect
    .poll(
      async () => {
        modules = await withinRuntimeDeadline(deadline, () => dk.modules());
        const boot = planSymbol(plan, 'DKRT_BOOT');
        const results = await withinRuntimeDeadline(deadline, () =>
          Promise.all(names.map((_, i) => word(session, boot + 40 + i * 4))),
        );
        return { names: modules.map((module) => module.name), results };
      },
      { timeout: 30_000, interval: 500 },
    )
    .toEqual({ names, results: names.map(() => 0) });
  const capacity = await bounded(() => dk.capacity());
  expect(capacity.FX_SLOTS).toEqual({ limit: slots, used: wet ? 1 : 0, largest: 0 });
  const settings = await bounded(() => readSettingsRecordIfPresent(dk));
  expect(settings === null ? null : settingsValue(settings, 'dk/fault')).toBeNull();
  await expect
    .poll(
      async () => {
        const status = await withinRuntimeDeadline(deadline, () => session.status());
        return { pending: status.cf.input_pending, scheduled: status.scheduled_inputs };
      },
      { timeout: Math.max(1, deadline - Date.now()), interval: 100, message: 'boot input has finished before pausing' },
    )
    .toEqual({ pending: 0, scheduled: 0 });
  await rpc(session, 'run.mode', { mode: 'paused' });
  const boot = planSymbol(plan, 'DKRT_BOOT');
  const fields = await Promise.all(Array.from({ length: 10 + names.length }, (_, i) => word(session, boot + i * 4)));
  // modules/core/runtime/abi.toml DkrtBootInfo: magic/version/safe/reason, count/loaded/UI, results.
  expect(fields.slice(0, 4)).toEqual([0x444b4254, 1, 0, 0]);
  expect(fields.slice(7, 10)).toEqual([names.length, names.length, 1]);
  expect(fields.slice(10)).toEqual(names.map(() => 0));
  const assigned = planSymbol(plan, 'DKRT_ASSIGN');
  expect(await word(session, assigned)).toBe(0x444b4153);
  expect(await word(session, assigned + 4)).toBe(1);
  expect(modules[0]!.types).toHaveLength(1);
  const chain = await rows(session, plan, slots);
  expect(chain.slice(wet ? 1 : 0).map((row) => row[0])).toEqual(Array(slots - (wet ? 1 : 0)).fill(FREE_PC));
  let oracle: FxExpectations | undefined;
  if (wet) {
    const effect = modules[1]!;
    expect(effect.contracts[2]).toEqual([1, 0]);
    expect(effect.contracts[1]).toEqual([0, 0]);
    expect(effect.rows).toBe(0);
    const base = planSymbol(plan, '_dl_area') + effect.dsp_off;
    const artifact = path.join(path.dirname(session.cardPath!), 'seed/digikit/modules/dyn-fx-gain.dkm');
    oracle = await analysisJson<FxExpectations>(
      'tools.patchkit.pk_dkrt_machine',
      ['fx-expectations', artifact, `0x${base.toString(16)}`],
      artifacts,
      `${tag}-expected-gain`,
    );
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
    modules: modules.map(({ name, contracts, types, rows: count, dsp_len }) => ({
      name,
      contracts,
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
  requireAudible = true,
): Promise<void> {
  const { session } = runtime;
  const guard = await bounded(() => session.client.guardReport());
  const file = path.join(artifacts, `${tag}.overlay.bin`);
  const overlay = await rpc(session, 'card.overlay', { path: file });
  overlays.push(file);
  const pcm = await rpc<{ non_finite: number; peak: number }>(session, 'pcm.tap', { action: 'report' });
  record(artifacts, `${tag}-closing`, { guard, overlay, pcm, status: await bounded(() => session.status()) });
  await bounded(() => expectNoGuardHits(session));
  expect(pcm.non_finite).toBe(0);
  if (requireAudible) expect(pcm.peak, 'input is audible').toBeGreaterThan(0.01);
  await bounded(() => session.expectNoFault(tag));
}

export interface GainFxSessionOptions {
  initial?: 'dry' | 'wet';
  /** Idle cases may be silent; guard, nonfinite, fault and overlay checks always remain mandatory. */
  requireAudible?: boolean | ((runtime: PatchedSession) => boolean);
}

/** Fixed default-emulator sine/gain fixtures, sharing the existing owned Session lifecycle. */
export function withGainFxSession(device: Device, scope: string, config: GainFxSessionOptions = {}) {
  let plan: RuntimePlan;
  let dry: RuntimeBaseline;
  let wet: RuntimeBaseline;
  let artifacts: string;
  let closed = 0;
  let watchOutput: string;
  const overlays: string[] = [];
  const epochs = new Set<number>();
  const rememberEpoch = (epoch: unknown) => {
    if (typeof epoch !== 'number') throw new Error('runtime status lacks epoch');
    epochs.add(epoch);
  };
  const slots = slotsFor(device);
  const collectEvidence = (runtime: PatchedSession, tag: string, requireAudible?: boolean) => {
    const required =
      requireAudible ??
      (typeof config.requireAudible === 'function' ? config.requireAudible(runtime) : (config.requireAudible ?? true));
    return evidence(runtime, artifacts, tag, overlays, required);
  };
  const fixture = withReusablePatchedSession(
    {
      ...options(device),
      env: async () => {
        artifacts = fs.mkdtempSync(path.join(os.tmpdir(), `digi-fx-reuse-${device}-`));
        console.info('FX safety artifacts:', artifacts);
        const prepared = await prepareRuntimePlanOffline({ device, firmware: FIRMWARE[device] }, artifacts, artifacts, [
          'DKRT_BOOT',
          'DKRT_ASSIGN',
          'fx.rows',
          '_dl_area',
        ]);
        plan = prepared.plan;
        watchOutput = path.join(artifacts, 'writewatch.jsonl');
        return analysisJson<Record<string, string>>(
          'tools.patchkit.pk_dkrt_machine',
          ['watch-env', prepared.planFile, watchOutput],
          artifacts,
          'watch-environment',
        );
      },
    },
    {
      prepareBaseline: async (session) => {
        rememberEpoch((await bounded(() => session.status())).epoch);
        dry = await baseline(session, plan, false, artifacts);
        wet = await baseline(session, plan, true, artifacts);
        return config.initial === 'wet' ? wet : dry;
      },
      finishCase: (runtime) => collectEvidence(runtime, `case-${++closed}`),
      afterClose: async () => {
        const report = await analysisJson<{ pass: boolean }>(
          'tools.coldfire.arena_safety',
          [
            'runtime-check',
            '--device',
            device,
            '--watch-output',
            watchOutput,
            '--min-emulators',
            String(epochs.size),
            ...overlays.flatMap((file) => ['--overlay', file]),
          ],
          artifacts,
          'lifetime-safety',
        );
        expect(report.pass, 'every emulator and exported overlay meets legacy safety policy').toBe(true);
      },
    },
    scope,
  );
  return {
    fixture,
    get plan() {
      return plan;
    },
    get artifacts() {
      return artifacts;
    },
    get slots() {
      return slots;
    },
    get dry() {
      return dry;
    },
    get wet() {
      return wet;
    },
    rememberEpoch,
    evidence: collectEvidence,
  };
}
