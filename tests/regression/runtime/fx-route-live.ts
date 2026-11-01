import { execFile } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import { expect } from 'vitest';
import { DkSession, EmuUsbTransport } from '@digi/dk-session';
import {
  button,
  MS,
  repoRoot,
  withinRuntimeDeadline,
  type Device,
  type RuntimeBaseline,
  type Session,
} from '@digi/test-support';
import { withReusablePatchedSession, type PatchedSession } from '../support.ts';
import { FIRMWARE, expectRuntimeUi, moduleArtifact, record } from './live.ts';
import { advance, analysisJson, bounded, evidence, options, word } from './fx-live.ts';
import {
  planSymbol,
  prepareRuntimePlanOffline,
  validateRuntimePlan,
  type PreparedRuntimePlan,
  type RuntimePlan,
} from './runtime-plan.ts';

const exec = promisify(execFile);
const root = repoRoot();
const RT = ['core/runtime', 'core/dynreg', 'core/dyndsp', 'core/dynfx'];
export const SINE = 'test/sine';
export const GAIN = 'test/dyn-fx-gain';
export const SEND = 'test/dyn-fx-gain-send';
export const SENDONLY = 'test/dyn-fx-gain-sendonly';
export const SEND3 = `${SEND}-3`;
export const FX = 'test/dyn-fx-gain-both';

export interface RouteProfile {
  fx_slots: number;
  send_slots: number;
  track_type_word: number;
  value_classes: number;
  selection: { copy_a: number; copy_b: number };
  settings: { copy_a: number; copy_b: number };
  kit_areas: [number, number][];
  fx: {
    kit_ptr: number;
    array_off: number;
    page_first: number;
    value_indices: number[];
    template_percent: number;
    template_bipolar: number;
    template_mix: number;
    store_off: number;
    store_halfwords: number;
    mark_index: number;
    tail_off: number;
    track_page: number;
    sound_off: number;
    sound_stride: number;
    send_template: number;
    send_indices: number[];
  };
}
export interface RouteCardPreset {
  /** Store paths (modules/m0.dkm, etc.) in boot order; every card contains the complete scenario inventory. */
  selection: string[];
  /** UTF-8 settings keys and hex-encoded byte values. */
  settings?: [string, string][];
}
export interface RouteFxSessionOptions {
  image: 'routes' | 'track' | 'track-base';
  initial: string;
  presets: Record<string, RouteCardPreset>;
  requireAudible?: boolean | ((runtime: PatchedSession) => boolean);
}

/** Legacy bounded panel tap, including release of every held chord key on failure. */
export async function tap(session: Session, key: string, settleMs = 300, held?: string): Promise<void> {
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
  if (settleMs) await advance(session, settleMs);
}

/** Preserve the exact decoded OLED image and hash for pixel comparisons. */
export async function shot(session: Session, artifacts: string, tag: string): Promise<string> {
  const frame = await bounded(() => session.client.framebufferGet({ hash: true, data: true }));
  if (!frame.sha256 || !frame.data_b64) throw new Error('route screenshot lacks OLED bytes or hash');
  const bytes = Buffer.from(frame.data_b64, 'base64');
  if (bytes.length !== 1024) throw new Error('route screenshot has an invalid OLED byte count');
  fs.writeFileSync(path.join(artifacts, `${tag}.oled`), bytes);
  record(artifacts, `${tag}-frame`, frame);
  return frame.sha256;
}

export async function readTextRows(session: Session, address: number, count: number, width: number): Promise<string[]> {
  if (!Number.isInteger(count) || count < 0 || !Number.isInteger(width) || width <= 0 || width % 4) {
    throw new Error('route text rows require a nonnegative count and a positive word-aligned width');
  }
  const result: string[] = [];
  for (let row = 0; row < count; row++) {
    const bytes = Buffer.alloc(width);
    for (let offset = 0; offset < width; offset += 4)
      bytes.writeUInt32BE(await word(session, address + row * width + offset), offset);
    result.push(bytes.toString('latin1').split('\0')[0]!);
  }
  return result;
}

/** Read protocol state while paused using the existing USB transport's legacy 400,000-instruction poll step. */
export async function routeProtocol<T>(runtime: PatchedSession, operation: (dk: DkSession) => Promise<T>): Promise<T> {
  const deadline = Date.now() + 30_000;
  const status = await withinRuntimeDeadline(deadline, () => runtime.session.status());
  if (status.mode !== 'paused') throw new Error('route protocol reads require a paused runtime');
  let active = true;
  const dk = new DkSession(
    new EmuUsbTransport(
      async <R>(method: string, params?: Record<string, unknown>) => {
        if (!active) throw new Error('route protocol is no longer active');
        const result = await withinRuntimeDeadline(deadline, () => runtime.session.client.call<R>(method, params));
        if (!active) throw new Error('route protocol is no longer active');
        return result;
      },
      { stepInstructions: 400_000 },
    ),
  );
  try {
    return await withinRuntimeDeadline(deadline, () => operation(dk));
  } finally {
    active = false;
    dk.cancelPending();
  }
}

/** Shared ready/boot observations; scenario assertions retain their own capacity and routing expectations. */
export async function inspectRouteBoot(
  runtime: PatchedSession,
  plan: RuntimePlan,
  expectedNames: string[],
  artifacts: string,
  tag: string,
  expectedResults = expectedNames.map(() => 0),
) {
  const { session, dk } = runtime;
  validateRuntimePlan(plan, session);
  await bounded(() => session.client.call('run.mode', { mode: 'free' }));
  await expectRuntimeUi(session, plan, artifacts);
  const boot = planSymbol(plan, 'DKRT_BOOT');
  let modules = await bounded(() => dk.modules());
  let results: number[] = [];
  await expect
    .poll(
      async () => {
        modules = await bounded(() => dk.modules());
        results = await Promise.all(expectedResults.map((_, i) => word(session, boot + 40 + i * 4)));
        return { names: modules.map((module) => module.name), results };
      },
      { timeout: 30_000, interval: 500 },
    )
    .toEqual({ names: expectedNames, results: expectedResults });
  const capacity = await bounded(() => dk.capacity());
  await expect
    .poll(
      async () => {
        const status = await bounded(() => session.status());
        return { pending: status.cf.input_pending, scheduled: status.scheduled_inputs };
      },
      { timeout: 30_000, interval: 100, message: 'route boot input has finished before pausing' },
    )
    .toEqual({ pending: 0, scheduled: 0 });
  await bounded(() => session.client.call('run.mode', { mode: 'paused' }));
  const fields = await Promise.all(Array.from({ length: 10 }, (_, i) => word(session, boot + i * 4)));
  expect(fields.slice(0, 4)).toEqual([0x444b4254, 1, 0, 0]);
  expect(fields.slice(7, 10)).toEqual([expectedResults.length, expectedNames.length, 1]);
  const status = await bounded(() => session.status());
  expect(status.mode).toBe('paused');
  expect(status.cf.input_pending).toBe(0);
  expect(status.scheduled_inputs).toBe(0);
  record(artifacts, tag, { fields, modules, results, capacity, status });
  return { fields, modules, results, capacity, epoch: status.epoch };
}

/** One exact image/plan/write-watch policy per owned fixture. Independent cards reload in the same daemon. */
export function withRouteFxSession(device: Device, scope: string, config: RouteFxSessionOptions) {
  if (!Object.hasOwn(config.presets, config.initial)) throw new Error(`unknown initial route preset ${config.initial}`);
  const inventory = config.image === 'routes' ? [SINE, GAIN, SEND, SENDONLY, SEND3] : [SINE, FX, `${FX}-2`, `${FX}-3`];
  const modules = [
    ...RT,
    ...(config.image === 'track-base' ? [] : ['core/fxpages']),
    ...(config.image === 'routes' ? ['core/digikit-menu'] : []),
  ];
  let prepared: PreparedRuntimePlan;
  let profile: RouteProfile;
  let artifacts: string;
  let watchOutput: string;
  let closed = 0;
  const baselines = new Map<string, RuntimeBaseline>();
  const overlays: string[] = [];
  const epochs = new Set<number>();
  const rememberEpoch = (epoch: unknown) => {
    if (typeof epoch !== 'number') throw new Error('route status lacks epoch');
    epochs.add(epoch);
  };
  const collectEvidence = async (runtime: PatchedSession, tag: string, requireAudible?: boolean) => {
    rememberEpoch((await bounded(() => runtime.session.status())).epoch);
    const required =
      requireAudible ??
      (typeof config.requireAudible === 'function' ? config.requireAudible(runtime) : (config.requireAudible ?? true));
    await evidence(runtime, artifacts, tag, overlays, required);
  };
  const fixture = withReusablePatchedSession(
    {
      ...options(device),
      env: async () => {
        artifacts = fs.mkdtempSync(path.join(os.tmpdir(), `digi-fx-${config.image}-${device}-`));
        console.info('FX route safety artifacts:', artifacts);
        prepared = await prepareRuntimePlanOffline(
          { device, firmware: FIRMWARE[device] },
          artifacts,
          artifacts,
          [
            'DKRT_BOOT',
            'DKRT_ASSIGN',
            'fx.rows',
            '_dl_area',
            ...(config.image === 'track' || config.image === 'track-base' ? ['dynreg_info'] : []),
            ...(config.image === 'track'
              ? ['fxpages_info', 'fxp_send_row', 'fxp_send_count', 'fxp_nload', 'DKRT_FXS']
              : []),
            ...(config.image === 'routes'
              ? [
                  'DK_FXR_PEND',
                  'DK_FXR_SET',
                  'DK_FXR_FLUSH',
                  'DK_POPUP',
                  'DK_POPUP_TEXT',
                  'DK_POPUPS',
                  'DK_APPLIES',
                  'DK_PANEL',
                  'DK_PANEL_N',
                  'DK_ROUTE_N',
                  'DK_ROUTE_ITEM',
                  'fxp_send_count',
                  'DKRT_FXS',
                  'fxpages_info',
                ]
              : []),
          ],
          { modules },
        );
        profile = await analysisJson<RouteProfile>(
          'tools.patchkit.pk_digikit_fxroutes',
          ['profile', '--device', device],
          artifacts,
          'profile',
        );
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
        const binary = path.join(root, 'out/native/target/nogen/release/plusdrive');
        if (!fs.existsSync(binary)) throw new Error(`prepare prebuilt plusdrive separately: ${binary}`);
        for (const [tag, preset] of Object.entries(config.presets)) {
          if (!/^[a-zA-Z0-9_-]+$/.test(tag)) throw new Error(`invalid route preset directory ${tag}`);
          if (
            new Set(preset.selection).size !== preset.selection.length ||
            preset.selection.some((file) => !inventory.some((_, index) => file === `modules/m${index}.dkm`))
          )
            throw new Error(`invalid route preset selection ${tag}`);
          const directory = path.join(session.runDir, tag);
          const seed = path.join(directory, 'seed');
          const destination = path.join(seed, 'digikit', 'modules');
          fs.mkdirSync(destination, { recursive: true });
          inventory.forEach((id, index) =>
            fs.copyFileSync(moduleArtifact(device, id), path.join(destination, `m${index}.dkm`)),
          );
          fs.writeFileSync(path.join(seed, 'digikit', 'selection.txt'), preset.selection.join('\n') + '\n');
          const cardImage = path.join(directory, 'card.img');
          const built = await exec(binary, ['build', seed, '-o', cardImage, '--device', device], {
            cwd: root,
            timeout: 30_000,
          });
          record(artifacts, `card-${tag}`, { stdout: built.stdout, stderr: built.stderr });
          if (preset.settings?.length) {
            const entries = path.join(directory, 'settings.json');
            fs.writeFileSync(entries, JSON.stringify(preset.settings.map(([key, value_hex]) => ({ key, value_hex }))));
            await analysisJson(
              'tools.patchkit.pk_digikit_fxroutes',
              ['seed-card', cardImage, '--device', device, '--entries', entries],
              artifacts,
              `settings-${tag}`,
            );
          }
          const boot = planSymbol(prepared.plan, 'DKRT_BOOT');
          if (!prepared.preparedImage) throw new Error('explicit route plan lacks prepared image identity');
          baselines.set(tag, {
            cardImage,
            preparedImage: prepared.preparedImage,
            guard: { protect: [{ lo: boot, hi: boot + 8, label: 'runtime boot identity' }], allow_pc: [] },
          });
        }
        return baselines.get(config.initial)!;
      },
      finishCase: (runtime) => collectEvidence(runtime, `case-${++closed}`),
      afterClose: async () => {
        const results = await Promise.allSettled([
          (async () => {
            const report = await analysisJson<{ pass: boolean }>(
              'tools.coldfire.arena_safety',
              [
                'runtime-check',
                '--device',
                device,
                '--policy',
                'fx-routes',
                '--watch-output',
                watchOutput,
                '--min-emulators',
                String(epochs.size),
                ...overlays.flatMap((file) => ['--overlay', file]),
              ],
              artifacts,
              'lifetime-safety',
            );
            expect(report.pass, 'every route emulator and exported overlay meets legacy safety policy').toBe(true);
          })(),
          analysisJson('tools.patchkit.pk_digikit_fxroutes', ['render-shots', artifacts], artifacts, 'rendered-shots'),
        ]);
        const errors = results.filter((result) => result.status === 'rejected').map((result) => result.reason);
        if (errors.length === 1) throw errors[0];
        if (errors.length > 1)
          throw new AggregateError(errors, 'route lifetime safety and screenshot rendering failed');
      },
    },
    scope,
  );
  return {
    fixture,
    get plan() {
      return prepared.plan;
    },
    get artifacts() {
      return artifacts;
    },
    get profile() {
      return profile;
    },
    get slots() {
      return profile.fx_slots;
    },
    get sendSlots() {
      return profile.send_slots;
    },
    baseline(tag: string): RuntimeBaseline {
      const baseline = baselines.get(tag);
      if (!baseline) throw new Error(`unknown or unprepared route preset ${tag}`);
      return baseline;
    },
    rememberEpoch,
    evidence: collectEvidence,
  };
}
