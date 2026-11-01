import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { DkSession, EmuUsbTransport } from '@digi/dk-session';
import { button, encoder, repoRoot, type Device, type Session } from '@digi/test-support';
import { describe, expect, onTestFinished, test } from 'vitest';
import { expectRuntimeBoot, withPatchedSession, type PatchedSession } from '../support.ts';
import { timePhase, timingContext } from '../timing.ts';

export const DEVICES = ['dt2', 'dn2'] as const;
export const FIRMWARE = { dt2: '1.17', dn2: '1.12' } as const;

export function liveOptions(device: Device, extra: { patchSet?: string; mirror?: boolean } = {}) {
  const binary = process.env[device === 'dt2' ? 'DIGI_EMUD_DT2' : 'DIGI_EMUD_DN2'];
  if (!binary) throw new Error(`DIGI_EMUD_${device.toUpperCase()} must name a preflighted daemon`);
  const card = device === 'dt2' ? path.join(repoRoot(), 'out/runs/patchkit/samples') : undefined;
  if (card && !fs.existsSync(card)) {
    throw new Error(
      `DT2 sample card missing: generate ${card} (tools.emulator.card_check.make_samples, run by pk_harness)`,
    );
  }
  return { device, firmware: FIRMWARE[device], binary, card, ...extra };
}

/** One test per device, each on its own fresh runtime daemon that has passed `expectRuntimeBoot`. */
export function runtimeTests(
  name: string,
  body: (runtime: PatchedSession) => Promise<void>,
  opts: { patchSet?: string; mirror?: boolean; timeout?: number; devices?: readonly Device[] } = {},
): void {
  for (const device of opts.devices ?? DEVICES) {
    describe(`${device.toUpperCase()} ${FIRMWARE[device]}`, () => {
      const handle = withPatchedSession(liveOptions(device, opts), name);
      test(
        name,
        async () => {
          await expectRuntimeBoot(handle.runtime);
          await timePhase('scenario.body', () => body(handle.runtime), timingContext(handle.runtime.session));
        },
        opts.timeout,
      );
    });
  }
}

export async function settleLive(session: Session, ms = 12_000): Promise<void> {
  await timePhase('settle.live', () => delay(ms), { ...timingContext(session), detail: `settle ${ms} ms` });
  expect((await session.status()).mode, 'the daemon runs freely').toBe('free');
}

/** The DN2 firmware needs a longer pre-install settle than the DT2. */
export function settleUi(session: Session): Promise<void> {
  return settleLive(session, session.options.device === 'dn2' ? 45_000 : 12_000);
}

/** Press, hold, release (also when the press fails), then let the UI settle. */
export async function tap(send: (down: boolean) => Promise<unknown>, holdMs: number, settleMs = 1200): Promise<void> {
  try {
    await send(true);
    await delay(holdMs);
  } finally {
    await send(false);
  }
  await timePhase('settle.input', () => delay(settleMs));
}

export function record(dir: string, name: string, value: unknown): void {
  fs.writeFileSync(path.join(dir, `${name}.json`), JSON.stringify(value, null, 2));
}

/** Prepared separately with patchkit pack; acceptance never builds a toolchain, module or daemon. */
export function moduleArtifact(device: Device, id: string): string {
  const directory = process.env.DIGI_RUNTIME_MODULES ?? path.join(repoRoot(), 'out/regression/modules');
  const artifact = path.join(directory, device, `${id.split('/').at(-1)}.dkm`);
  if (!fs.existsSync(artifact)) throw new Error(`prepare ${id} for ${device} with patchkit pack: missing ${artifact}`);
  return artifact;
}

/** The debug module: the log (LOG, PEEK, LOGCFG) exists only while it is loaded, so a test that reads the log selects it next to the module it tests. */
export const DEBUG_MODULE = 'digikit/debug';

/** Native USB upload of prepared modules, one selection with all of them (in this order), one restart, then wait for their registration. */
export async function installModules(dk: DkSession, modules: { id: string; artifact: string }[]): Promise<void> {
  await timePhase(
    'module.install',
    async () => {
      const paths: string[] = [];
      for (const { id, artifact } of modules) {
        const bytes = fs.readFileSync(artifact);
        const storePath = `modules/${id.split('/').at(-1)}.dkm`;
        await timePhase('module.upload', () => dk.put(storePath, bytes));
        expect(await dk.verifyFile(storePath, bytes), 'fresh card read matches prepared module bytes').toBe(true);
        paths.push(storePath);
      }
      await dk.select(paths);
      await timePhase('protocol.restart', () => dk.restart());
      await timePhase('protocol.waitForBoot', () => dk.waitForBoot());
      await expect
        .poll(async () => (await dk.modules()).map((entry) => entry.name), { timeout: 30_000, interval: 500 })
        .toEqual(expect.arrayContaining(modules.map((m) => m.id)));
    },
    { detail: modules.map((m) => m.id).join(' ') },
  );
}

/** One prepared module alone (a test that does not read the log). */
export function installModule(dk: DkSession, id: string, artifact: string): Promise<void> {
  return installModules(dk, [{ id, artifact }]);
}

/** The debug module and the modules `ids`, in one selection and one restart. */
export function installWithDebug({ session, dk }: PatchedSession, ...ids: string[]): Promise<void> {
  const device = session.options.device;
  return installModules(
    dk,
    [DEBUG_MODULE, ...ids].map((id) => ({ id, artifact: moduleArtifact(device, id) })),
  );
}

/** New protocol and USB receive state; deliberately retain the daemon/card/boot history. */
export function liveProtocol(session: Session): DkSession {
  return new DkSession(new EmuUsbTransport((method, params) => session.client.call(method, params)), {
    timeoutMs: 20_000,
    chunk: 4096,
  });
}

const artifactDirectories = new WeakMap<Session, Map<string, string>>();

export function liveArtifacts(session: Session, workflow: string): string {
  let workflows = artifactDirectories.get(session);
  if (!workflows) {
    workflows = new Map();
    artifactDirectories.set(session, workflows);
  }
  const existing = workflows.get(workflow);
  if (existing) return existing;
  const root = path.join(os.tmpdir(), 'digi-regression', workflow, session.options.device);
  fs.mkdirSync(root, { recursive: true });
  const dir = fs.mkdtempSync(path.join(root, 'run-'));
  workflows.set(workflow, dir);
  console.info('runtime artifacts:', dir);
  try {
    // Removed when the requesting test passes; kept on failure, and when called outside a test.
    onTestFinished(({ task }) => {
      workflows.delete(workflow);
      if (task.result?.state === 'pass') fs.rmSync(dir, { recursive: true, force: true });
      else console.info('artifacts kept:', dir);
    });
  } catch {
    // Outside a test: keep the directory.
  }
  return dir;
}

export function liveCheck(name: string, ok: unknown, detail: unknown = ''): void {
  expect(Boolean(ok), `${name}: ${String(detail)}`).toBe(true);
}

/** 0.1 seconds at 44.1 kHz exercises the converter's resampling path. */
export function sampleWav(channels: 1 | 2): Uint8Array {
  const frames = 4410;
  const pcm = Buffer.alloc(frames * channels * 2);
  for (let i = 0; i < frames; i++) {
    pcm.writeInt16LE(Math.round(12000 * Math.sin((2 * Math.PI * 440 * i) / 44100)), i * channels * 2);
    if (channels === 2) pcm.writeInt16LE(Math.round(9000 * Math.sin((2 * Math.PI * 660 * i) / 44100)), i * 4 + 2);
  }
  const h = Buffer.alloc(44);
  h.write('RIFF', 0);
  h.writeUInt32LE(36 + pcm.length, 4);
  h.write('WAVEfmt ', 8);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20);
  h.writeUInt16LE(channels, 22);
  h.writeUInt32LE(44100, 24);
  h.writeUInt32LE(44100 * channels * 2, 28);
  h.writeUInt16LE(channels * 2, 32);
  h.writeUInt16LE(16, 34);
  h.write('data', 36);
  h.writeUInt32LE(pcm.length, 40);
  return Uint8Array.from(Buffer.concat([h, pcm]));
}

/** ColdFire instructions per emulated millisecond. */
const MS = 132_000;

/** Input scheduling uses elapsed, the daemon's own input clock, not wall time. */
export function panel(session: Session) {
  async function elapsed(): Promise<number> {
    const value = (await session.status()).elapsed;
    if (typeof value !== 'number' || !Number.isSafeInteger(value)) throw new Error('input requires status.elapsed');
    return value;
  }
  async function until(target: number): Promise<void> {
    await expect.poll(elapsed, { timeout: 20_000, interval: 50 }).toBeGreaterThanOrEqual(target);
  }
  async function settle(ms: number): Promise<void> {
    await until((await elapsed()) + ms * MS);
  }
  async function tap(name: string | number, settleMs = 80): Promise<void> {
    const result = (await session.client.inputTap(button(name), 20 * MS)) as { release_at?: number };
    if (!Number.isSafeInteger(result.release_at)) throw new Error('input.tap must report release_at');
    await until(result.release_at! + 5 * MS);
    await settle(settleMs);
  }
  async function chord(held: string, key: string | number): Promise<void> {
    await session.client.inputButton(button(held), true);
    try {
      await settle(5);
      await tap(key);
    } finally {
      await session.client.inputButton(button(held), false);
    }
    await settle(300);
  }
  async function turn(name: string, detents: number): Promise<void> {
    await session.client.inputTurn(encoder(name), detents);
    await settle(200);
  }
  return { tap, chord, turn, settle };
}

/** Read-only ColdFire and DSP memory access over the control socket. */
export function memory(session: Session) {
  async function cf(addr: number): Promise<number> {
    const reply = (await session.client.call('mem.cf.peek', { addr })) as { value: number };
    return reply.value >>> 0;
  }
  async function words(addr: number, count: number): Promise<number[]> {
    const values = [];
    for (let i = 0; i < count; i++) values.push(await cf(addr + i * 4));
    return values;
  }
  async function dsp(addr: number, len: number) {
    return (await session.client.call('dsp.peek', { addr, len })) as {
      addr: number;
      len: number;
      hex: string;
      unknown: number;
    };
  }
  return { cf, words, dsp };
}
