import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { asciiOf, hasCap, type DkLogEvent, type DkSession } from '@digi/dk-session';
import { CMD, LOG_KIND } from '@digi/dk-session/gen_proto';
import { button, MS, repoRoot, type Device, type RuntimeBaseline, type Session } from '@digi/test-support';
import { parse } from 'smol-toml';
import { describe, expect } from 'vitest';
import { expectNoGuardHits, withReusablePatchedSession, type PatchedSession, type RuntimeCase } from '../support.ts';
import { applyOverlay, overlayWrites, runtimeSafety, runtimeWatchEnv } from './write-safety.ts';
import { bootInfo, buildCard, selRecords, type CardOptions, type SelRecord } from './card-kit.ts';
import { advance, options, rpc, word } from './fx-live.ts';
import { withPausedFxProtocol } from './fx-session.ts';
import { DEVICES, FIRMWARE, FULL_TIER, moduleArtifact } from './live.ts';
import { openDigikitMenu, readAboutRows, readSmallRing, RING_SYMBOLS } from './runtime-menu.ts';
import { planSymbol, prepareRuntimePlanOffline, validateRuntimePlan, type RuntimePlan } from './runtime-plan.ts';

/**
 * Local helpers of the debug, card, crash, safe and cut scenarios (tests/regression/runtime/{debug-module,card-safety,safe-mode,crash-capture,debug-guard}).
 * Candidates for card-kit.ts: `cardTests` (a daemon that boots one card image after the other), `waitReset`, `crashRecord`, `OPS`.
 */

/** The runtime image of the Python scenarios (`B.RT`). */
export const RT = ['core/runtime', 'core/dynreg', 'core/dyndsp'];
export const E_NOSTORE = -61;
export const E_INUSE = -62;
export const E_PEEK = -63;
/** A time out of the 32-bit words of an event or a slot: no value. */
export const NONE = 0xffffffff;
export const CARD_TIMEOUT = 1_200_000;
/** A runtime variable the card scenarios read or poke. */
const CARD_SYMBOLS = ['DKRT_BOOT', 'DKRT_CARD', 'DKRT_CRASH', 'DKRT_MAILBOX', 'DKRT_STATE', ...RING_SYMBOLS];

export const be32 = (n: number) => Buffer.from([n >>> 24, (n >>> 16) & 255, (n >>> 8) & 255, n & 255]);

/** Deterministic bytes with every value (the 8-in-7 packing needs the high bits). */
export function blob(n: number, seed: number): Buffer {
  const out = Buffer.alloc(n);
  let x = (seed * 2654435761) >>> 0 || 1;
  for (let i = 0; i < n; i++) {
    x = (Math.imul(x, 1664525) + 1013904223) >>> 0;
    out[i] = x >>> 24;
  }
  return out;
}

/** Store commands that a card without a usable store refuses; the bodies are never valid uploads. */
export const OPS = {
  begin: [CMD.BEGIN, Buffer.concat([Buffer.from('modules/x.dkm\0'), be32(10), Buffer.alloc(32)])],
  delete: [CMD.DELETE, Buffer.from('modules/x.dkm\0')],
  mkdir: [CMD.MKDIR, Buffer.from('samples\0')],
  clear: [CMD.CLEAR, be32(0x434c5221)],
  list: [CMD.LIST, be32(0)],
} as const;

/** The status of each named store command. */
export async function opStatuses(dk: DkSession, names: (keyof typeof OPS)[]): Promise<number[]> {
  const out: number[] = [];
  for (const name of names) out.push((await dk.request(OPS[name][0], OPS[name][1])).status);
  return out;
}

// ---- device data --------------------------------------------------------------------------------------------------------------------

const PROFILE: Record<Device, string> = { dt2: 'digitakt-ii', dn2: 'digitone-ii' };

interface DeviceFile {
  digikit: { peek: [number, number][]; dsp_area_bytes: number };
  limits: Record<string, number>;
  card: { store: { header_b_sector: number } };
}

/** The parts of devices/<profile>.toml that the scenarios compare with (Python `pk_device`). */
export const deviceFile = (device: Device) =>
  parse(fs.readFileSync(path.join(repoRoot(), 'devices', `${PROFILE[device]}.toml`), 'utf8')) as unknown as DeviceFile;

// ---- the log ------------------------------------------------------------------------------------------------------------------------

/** Every event the device's log holds: over SysEx while the debug module is loaded, else the runtime's own small ring from memory. */
export async function logEvents({ session, dk }: PatchedSession, plan: RuntimePlan): Promise<DkLogEvent[]> {
  return hasCap(await dk.info(), 'LOG') ? dk.readLog() : readSmallRing(session, plan);
}

export const ofKind = (events: DkLogEvent[], kind: number) => events.filter((e) => e.kind === kind);

/** The events of a CPU crash as the boot after it logged them (Python `dklog.crash_record`); null without a CRASH_FOUND. */
export function crashRecord(events: DkLogEvent[]) {
  const at = events.map((e) => e.kind === LOG_KIND.CRASH_FOUND).lastIndexOf(true);
  if (at < 0) return null;
  const first = events[at]!;
  const parts = new Set<number>([
    LOG_KIND.CRASH_MOD,
    LOG_KIND.CRASH_REG,
    LOG_KIND.CRASH_STACK,
    LOG_KIND.CRASH_TAIL,
    LOG_KIND.DSP_DIAG,
  ]);
  const rest: DkLogEvent[] = [];
  for (const e of events.slice(at + 1)) {
    if (!parts.has(e.kind)) break;
    rest.push(e);
  }
  const kind = (k: number) => rest.filter((e) => e.kind === k);
  const mod = kind(LOG_KIND.CRASH_MOD).find((e) => e.args[0] !== 0x100);
  return {
    corrupt: first.args[4] === 0xbad,
    pc: first.args[0]!,
    vector: first.args[1]! & 0xff,
    a7: first.args[3]!,
    regs: kind(LOG_KIND.CRASH_REG).flatMap((e) => e.args.slice(1).filter((_, i) => e.args[0]! + i < 16)),
    stack: kind(LOG_KIND.CRASH_STACK).flatMap((e) => e.args.slice(1)),
    /** Pairs of CRASH_TAIL events make one event of the boot before. */
    tail: Math.floor(kind(LOG_KIND.CRASH_TAIL).length / 2),
    module: mod ? { index: mod.args[0] === NONE ? null : mod.args[0]!, name: asciiOf(...mod.args.slice(2, 5)) } : null,
  };
}

// ---- emulated time, keys and PCM ---------------------------------------------------------------------------------------------------

/** Wait until the DSP has run `blocks` more frames (emulated time, not wall time). */
export async function waitFrames({ session }: { session: Session }, blocks: number): Promise<void> {
  const frames = async () => Number(((await session.status()).audio as { frames?: number } | null)?.frames ?? 0);
  const start = await frames();
  await expect
    .poll(async () => (await frames()) - start, { timeout: 900_000, interval: 1000, message: `${blocks} DSP blocks` })
    .toBeGreaterThanOrEqual(blocks);
}

/** The ABOUT page of the DIGIKIT menu, opened through the panel in a session that runs freely (keys held in emulated time). */
export async function openAbout(runtime: { session: Session }, plan: RuntimePlan): Promise<string[]> {
  const tap = async (name: string, settleMs = 1200) => {
    await runtime.session.client.inputTap(button(name), 100 * MS);
    await waitFrames(runtime, Math.round(settleMs * 1.5));
  };
  await openDigikitMenu(tap);
  await tap('YES', 3000); // ABOUT is the first row
  return readAboutRows(runtime.session, plan);
}

/** Pause the run and give `op` a protocol session that steps the emulator (the scenarios that press keys with stepped time). */
export async function pausedProtocol<T>(runtime: PatchedSession, op: (dk: DkSession) => Promise<T>): Promise<T> {
  await rpc(runtime.session, 'run.mode', { mode: 'paused' });
  return withPausedFxProtocol(runtime, op);
}

/** Float32 samples (interleaved) of a RIFF IEEE-float WAV, as `hook_ab.read_wav`. */
export function readFloatWav(file: string): Float32Array {
  const b = fs.readFileSync(file);
  if (b.toString('latin1', 0, 4) !== 'RIFF' || b.toString('latin1', 8, 12) !== 'WAVE') throw new Error(`${file}: not a WAV`);
  let fmt = 0;
  for (let pos = 12; pos + 8 <= b.length; ) {
    const tag = b.toString('latin1', pos, pos + 4);
    const size = b.readUInt32LE(pos + 4);
    if (tag === 'fmt ') fmt = b.readUInt16LE(pos + 8);
    else if (tag === 'data') {
      if (fmt !== 3) throw new Error(`${file}: expected an IEEE float WAV, format ${fmt}`);
      const n = Math.floor(Math.min(size, b.length - pos - 8) / 4);
      return Float32Array.from({ length: n }, (_, i) => b.readFloatLE(pos + 8 + 4 * i));
    }
    pos += 8 + size + (size & 1);
  }
  throw new Error(`${file}: no data chunk`);
}

/** RMS of each full window of `win` samples. */
export function windowRms(samples: Float32Array, win = 4800): number[] {
  const out: number[] = [];
  for (let i = 0; i + win < samples.length; i += win) {
    let sum = 0;
    for (let k = i; k < i + win; k++) sum += samples[k]! * samples[k]!;
    out.push(Math.sqrt(sum / win));
  }
  return out;
}

/** Run in steps (the run must be paused) until the word at `address` stops changing; false when it never does within `limitMs`. */
export async function settleHang(session: Session, address: number, limitMs = 9000): Promise<boolean> {
  for (let i = 0; i < Math.floor(limitMs / 400); i++) {
    const before = await word(session, address);
    await advance(session, 400);
    if ((await word(session, address)) === before) return true;
  }
  return false;
}

/**
 * Port of `Rig.wait_reset` for a paused session: step until the guest soft-resets (a crash that the runtime's hook turned into a reset) and the controller's
 * cold boot is loaded, then until it is ready. No step is issued once the reset is seen (a step during the cold boot runs against the new machine with the
 * old budget).
 */
export async function waitReset({ session }: { session: Session }, timeoutMs = 900_000): Promise<void> {
  const { client } = session;
  client.drainEvents();
  let resetSeen = false;
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    if (resetSeen) {
      // the controller notices that its worker has finished only when a command wakes it: a status request is one
      await delay(200);
      await session.status().catch(() => undefined);
    } else await client.step(400_000, { timeout_wall_s: 30 }).catch(() => delay(500)); // steps are refused during a cold boot
    for (const ev of client.drainEvents()) {
      if (ev.kind === 'cpu.reset') {
        if (ev.restart === 'unsupported') throw new Error('the emulator cannot restart this build');
        resetSeen = true;
      }
      if (ev.kind === 'firmware.loaded') {
        if (!ev.ok) throw new Error(`reset: the cold boot failed ${JSON.stringify(ev)}`);
        await client.watchdogConfig(session.options.stallMs ?? 1000);
        for (let i = 0; i < 200 && !(await session.status()).cf.ready; i++)
          await client.step(400_000, { timeout_wall_s: 30 });
        return;
      }
    }
  }
  throw new Error(`no reset within ${timeoutMs / 1000} s`);
}

// ---- cards: one daemon, one card image after the other ----------------------------------------------------------------------------

export interface CardHarnessOptions {
  /** Runtime image roots (default `RT`). */
  image?: string[];
  /** Plan symbols besides `CARD_SYMBOLS`. */
  symbols?: string[];
  /** Daemon environment besides the write watch. */
  env?: Record<string, string>;
  /** `small`: the first card is a blank 1 MiB file (a test sets `DIGI_CARD_IDENTITY=small`); default a blank store card. */
  initial?: 'blank' | 'small';
  /** Uploads and project writes are part of the scenario: the write watch is checked, the card writes are not (`common_checks_upload`). */
  uploads?: boolean;
  /** A CPU exception is part of the scenario: faults of the session are not an error. */
  faults?: boolean;
  devices?: readonly Device[];
  /** Full tier only. */
  full?: boolean;
}

export interface CardHarness {
  readonly device: Device;
  readonly plan: RuntimePlan;
  /** The case of the card booted last. */
  readonly runtime: RuntimeCase;
  /** The image file of the card booted last. */
  readonly cardImage: string;
  /** A module for a card: store path `modules/<as>.dkm` (default the last part of `id`) and its prepared .dkm. */
  mod(id: string, as?: string): { name: string; file: string };
  /** A file below `cfg/` for a card (the fault and crash modules read their delay from it). */
  cfg(name: string, bytes: Uint8Array): { name: string; file: string };
  /** A card in a fresh directory of the run directory (plusdrive build + the records of `o`). */
  card(o?: Omit<CardOptions, 'dir'>): Promise<string>;
  /** Boot the runtime on this card image in the same daemon; the previous case's evidence is collected first. */
  boot(cardImage: string, o?: { deliverExceptions?: boolean }): Promise<RuntimeCase>;
  /** Poll until the UI loop was reached (and the boot is safe mode when `safe`); returns the boot's fields. */
  ready(safe?: boolean): Promise<Awaited<ReturnType<typeof bootInfo>>>;
  /** The card the guest left: this card with the overlay of its writes laid over a copy (Python `card_after`). */
  after(): Promise<string>;
  /** The two selection record copies as the card holds them now. */
  records(): Promise<(SelRecord | null)[]>;
  /** The sectors of the store that the guest has written. */
  storeWrites(): Promise<number[]>;
}

/**
 * One daemon per device and describe block, one test per case: a case boots a card image with `boot` (the first card comes from `initial`). The write
 * watch of the daemon and the overlay of every case are checked when the daemon has closed (Python `common_checks`; `uploads`: `common_checks_upload`).
 */
export function cardTests(label: string, o: CardHarnessOptions, define: (h: CardHarness) => void): void {
  for (const device of o.devices ?? DEVICES) {
    describe.skipIf(o.full === true && !FULL_TIER)(`${device.toUpperCase()} ${FIRMWARE[device]}: ${label}`, () => {
      define(cardHarness(device, label, o));
    });
  }
}

function cardHarness(device: Device, scope: string, o: CardHarnessOptions): CardHarness {
  let prepared: Awaited<ReturnType<typeof prepareRuntimePlanOffline>>;
  let artifacts: string;
  let watchOutput: string;
  let image = '';
  let cases = 0;
  const overlays: string[] = [];
  const epochs = new Set<number>();
  const symbols = [...CARD_SYMBOLS, ...(o.symbols ?? [])];

  const baseline = (cardImage: string, deliverExceptions?: boolean): RuntimeBaseline => {
    if (!prepared.preparedImage) throw new Error('the explicit runtime plan lacks a prepared image identity');
    const boot = planSymbol(prepared.plan, 'DKRT_BOOT');
    return {
      cardImage,
      preparedImage: prepared.preparedImage,
      guard: { protect: [{ lo: boot, hi: boot + 8, label: 'runtime boot identity' }], allow_pc: [] },
      deliverExceptions,
    };
  };
  const exportOverlay = async (session: Session, tag: string) => {
    const file = path.join(artifacts, `${tag}.overlay.bin`);
    await rpc(session, 'card.overlay', { path: file });
    return file;
  };
  const freshDir = (session: Session) => fs.mkdtempSync(path.join(session.runDir, 'card-'));

  const fixture = withReusablePatchedSession(
    {
      ...options(device),
      env: async () => {
        artifacts = fs.mkdtempSync(path.join(os.tmpdir(), `digi-card-${device}-`));
        prepared = await prepareRuntimePlanOffline({ device, firmware: FIRMWARE[device] }, artifacts, artifacts, symbols, {
          modules: o.image ?? RT,
        });
        watchOutput = path.join(artifacts, 'writewatch.jsonl');
        return { ...runtimeWatchEnv(prepared.planFile, watchOutput), ...o.env };
      },
    },
    {
      prepareBaseline: async (session) => {
        epochs.add((await session.status()).epoch as number);
        const dir = freshDir(session);
        if (o.initial === 'small') {
          image = path.join(dir, 'small-card.img');
          fs.writeFileSync(image, Buffer.alloc(1 << 20)); // blank; the identity, not the file, sets the capacity
        } else image = await buildCard(device, { dir });
        return baseline(image);
      },
      finishCase: async (runtime) => {
        epochs.add((await runtime.session.status()).epoch as number);
        overlays.push(await exportOverlay(runtime.session, `case-${++cases}`));
        await expectNoGuardHits(runtime.session);
        if (!o.faults) await runtime.session.expectNoFault('card case');
      },
      afterClose: async () => {
        const report = runtimeSafety(device, watchOutput, overlays, Math.max(1, epochs.size));
        expect(report.write_events, 'write-watch on cf.expansion: no stray write').toBe(0);
        expect(report.flush_hits, 'flush check: no hit').toBe(0);
        if (!o.uploads) expect(report.pass, 'card writes only in the selection record sectors').toBe(true);
      },
    },
    scope,
  );

  const h: CardHarness = {
    device,
    get plan() {
      return prepared.plan;
    },
    get runtime() {
      return fixture.runtime;
    },
    get cardImage() {
      return image;
    },
    mod: (id, as) => ({ name: `modules/${as ?? id.split('/').at(-1)}.dkm`, file: moduleArtifact(device, id) }),
    cfg(name, bytes) {
      const file = path.join(artifacts, `cfg-${name}`);
      fs.writeFileSync(file, bytes);
      return { name: `cfg/${name}`, file };
    },
    card: (c = {}) => buildCard(device, { ...c, dir: freshDir(fixture.runtime.session) }),
    async boot(cardImage, extra = {}) {
      const next = await fixture.reload(baseline(cardImage, extra.deliverExceptions));
      validateRuntimePlan(prepared.plan, next.session);
      epochs.add(next.epoch);
      image = cardImage;
      return next;
    },
    async ready(safe = false) {
      const { session } = fixture.runtime;
      await expect
        .poll(
          async () => {
            const b = await bootInfo(session, prepared.plan);
            return b.uiReached === 1 && b.safe === (safe ? 1 : 0);
          },
          { timeout: 600_000, interval: 1000, message: `the UI loop was reached (safe mode: ${safe})` },
        )
        .toBe(true);
      return bootInfo(session, prepared.plan);
    },
    async after() {
      const out = path.join(freshDir(fixture.runtime.session), 'card.img');
      applyOverlay(image, await exportOverlay(fixture.runtime.session, `after-${++cases}`), out);
      return out;
    },
    async records() {
      const { session } = fixture.runtime;
      return selRecords(session, image, device, await exportOverlay(session, `records-${++cases}`));
    },
    async storeWrites() {
      const { session } = fixture.runtime;
      return overlayWrites(await exportOverlay(session, `writes-${++cases}`), device).store_sectors;
    },
  };
  return h;
}
