import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { DkSession, EmuUsbTransport, frame, parseReply, steppedClock } from '@digi/dk-session';
import { RpcError } from '@digi/control-client';
import { resolveInputs, type Device, type RuntimeBaseline, type Session } from '@digi/test-support';
import { describe, expect } from 'vitest';
import { expectNoGuardHits } from '../free-region-guard.ts';
import { withReusablePatchedSession } from '../reusable-session.ts';
import type { RuntimeCase } from '../test-runtime.ts';
import { withWaitApi } from '../wait-api.ts';
import { bootInfo, bootInfoLoaded, pokeKnob } from './guest-mailbox.ts';
import { buildCard, cardView, storeList } from './card-store.ts';
import { selRecords } from './card-record.ts';
import type { CardRig, CardRigOptions } from './card-rig-api.ts';
import type { CardSpec } from './card-spec.ts';
import { cachedCard, cachedCardId, cardSpecKey, fileSha } from './card-cache.ts';
import { uploadStore } from './device-data.ts';
import { bounded, rpc } from './guest-access.ts';
import { options } from './devices.ts';
import { SETTLE } from './panel-waits.ts';
import { drainInputs, withPausedFxProtocol } from './fx-session.ts';
import { refusePausedProtocol } from './paused-protocol.ts';
import { emulatorDk, pollEmulated } from './waits.ts';
import { record } from './artifacts.ts';
import { FIRMWARE, FULL_TIER, claimRunDir, selectDevices, skipEmptyHalf } from './devices.ts';
import { moduleArtifact, moduleStorePath } from './module-artifact.ts';
import { neededServices, portFile, requireCoupledForServices, servicePath } from './service-files.ts';
import { expectRuntimeUi } from './ready.ts';
import {
  planSymbol,
  prepareRuntimePlanListedOffline,
  prepareRuntimePlanOffline,
  validateRuntimePlan,
  type PreparedRuntimePlan,
} from './runtime-plan.ts';
import {
  cardAfter,
  cardOverlay,
  linkCard,
  overlayFile,
  overlayRecords,
  overlayWrites,
  runtimeSafety,
  runtimeWatchEnv,
  withoutOwn,
} from './write-safety.ts';
import { systemPackageSectors } from './system-package.ts';

/**
 * The one card fixture of the runtime device tests (settings, round trip, equiv, debug, card, crash, safe mode and upload): an owned daemon
 * that cold-boots one card image after the other, with the write watch of the runtime proof. A test boots a card (`boot`, `load`, `pausedBoot`),
 * steps or pauses as it needs, and reads what the guest left (`after`, `records`, `storeWrites`). When the daemon has closed, the write watch,
 * the flush check and the card writes of every case are checked (Python `common_checks`, or `common_checks_upload` with `uploads`).
 */

/** The test knob `DKRT_STORE_CAP` keeps every sector from here to header B out of the allocations (a DN2 store of 20 GiB then fits INFO's 32 bits). */
export const CAP_SECTORS = 0x40000;

const WAIT_RESET_STEP_INSNS = 400_000;
const WAIT_RESET_STEPS = 200;

/** One describe per device (`label` in its title) with a rig inside. */
export function cardRigs(label: string, o: CardRigOptions, define: (rig: CardRig) => void): void {
  const selected = selectDevices(o.devices);
  skipEmptyHalf(selected, label);
  for (const device of selected) {
    describe.skipIf((o.full === true && !FULL_TIER) || o.skip?.[device] !== undefined)(
      `${device.toUpperCase()} ${FIRMWARE[device]}: ${label}`,
      () => {
        define(cardRig(device, label, o));
      },
    );
  }
}

/**
 * One daemon with the write-watch environment of the runtime proof, to be called inside a describe block: every test case starts on the first card
 * and loads its own cards in the same daemon. After the daemon closes, every sequenced watch log and every exported overlay is checked.
 */
export function cardRig(device: Device, scope: string, o: CardRigOptions = {}): CardRig {
  let prepared: PreparedRuntimePlan;
  let artifacts = '';
  let watchOutput = '';
  let image = '';
  let clone = '';
  let closed = 0;
  let seq = 0;
  let cap: number | undefined;
  let stepping: { dk: DkSession; transport: EmuUsbTransport } | undefined;
  const overlays: string[] = [];
  const cardServices = new Set<string>();
  const epochs = new Set<number>();
  const extra = new Set(o.storeExtra ?? []);
  const symbols = [...(o.plain ? [] : ['DKRT_BOOT', 'DKRT_HOST_DIR']), ...(o.symbols ?? [])];

  const baseline = (cardImage: string, deliverExceptions = o.deliverExceptions): RuntimeBaseline => {
    if (o.roots && !prepared.preparedImage)
      throw new Error('the explicit runtime plan lacks a prepared image identity');
    const boot = o.plain ? 0 : planSymbol(prepared.plan, 'DKRT_BOOT');
    return {
      cardImage,
      preparedImage: prepared.preparedImage,
      guard: { protect: o.plain ? [] : [{ lo: boot, hi: boot + 8, label: 'runtime boot identity' }], allow_pc: [] },
      deliverExceptions,
      initialButtons: o.initialButtons,
      relaxedReady: o.relaxFs,
      paused: o.hold,
    };
  };
  const freshDir = (session: Session, tag = 'card') => {
    fs.mkdirSync(session.runDir, { recursive: true });
    return fs.mkdtempSync(path.join(session.runDir, `${tag}-`));
  };
  /** The overlay file of the session's card writes: by default only what the guest wrote since the load (`raw`: the daemon's whole overlay). */
  const exportOverlay = async (session: Session, tag: string, raw = false) => {
    const file = path.join(artifacts, `${tag}.overlay.bin`);
    await rpc(session, 'card.overlay', { path: file });
    if (!raw) fs.writeFileSync(file, withoutOwn(fs.readFileSync(file), cardOverlay(image)));
    return file;
  };
  const mod = (id: string, as?: string) => ({
    name: moduleStorePath(device, id, as),
    file: moduleArtifact(device, id),
  });
  /**
   * A card of `spec`. It is built once into the card cache under a name of its inputs, so the same spec gives the same file in every run and
   * the daemon finds its snapshot. A card on a `base` outside the cache has no stable name: it is built in a fresh directory of the run dir.
   */
  const build = async (session: Session, tag: string, spec: CardSpec = {}) => {
    const { modules = [], name: _name, ...rest } = spec;
    const files = modules.map((m) => (typeof m === 'string' ? mod(m) : m));
    // the services that these modules need go on the card too, as packed, with the port file of the firmware (`service-files.ts`)
    // only the files of `modules/` are modules: samples, other data and the service files of a test have no NEEDS
    const needed = neededServices(files.filter((m) => m.name.startsWith('modules/')).map((m) => m.file));
    const services: { name: string; file: string }[] = needed
      .filter((n) => !files.some((m) => m.name === servicePath(n.service)))
      .map((n) => ({ name: servicePath(n.service), file: moduleArtifact(device, n.moduleId) }));
    // the port file the services resolve their firmware names from (a test that lists its own port file keeps it)
    if (
      (services.length || files.some((m) => m.name.startsWith('services/'))) &&
      !files.some((m) => m.name.startsWith('ports/'))
    )
      services.push(portFile(device));
    requireCoupledForServices([...files, ...services], o.dsp);
    for (const n of needed) cardServices.add(n.service);
    const options = {
      ...(o.project ? { syx: resolveInputs(device, FIRMWARE[device]).syx } : {}),
      ...rest,
      modules: [...files, ...services],
    };
    const base = options.base ? cachedCardId(options.base) : undefined;
    if (options.base && !base) return buildCard(device, { ...options, dir: freshDir(session, tag) });
    const key = cardSpecKey(device, { ...options, base });
    return cachedCard(key, (work) => buildCard(device, { ...options, dir: work, name: 'card' }));
  };

  const fixture = withReusablePatchedSession(
    {
      ...options(device),
      runDirClaim: () => claimRunDir(device),
      protocolCall: (call, session) => refusePausedProtocol(call, async () => (await session.status()).mode),
      ...(o.stallMs !== undefined ? { stallMs: o.stallMs } : {}),
      dsp: o.initial === 'small' ? 'cold' : o.dsp,
      startEmpty: o.startEmpty,
      env: async () => {
        artifacts = fs.mkdtempSync(
          path.join(os.tmpdir(), `digi-${scope.split(':')[0]!.replace(/\W+/g, '-')}-${device}-`),
        );
        console.info('card scenario artifacts:', artifacts);
        const target = { device, firmware: FIRMWARE[device] };
        prepared = o.roots
          ? await prepareRuntimePlanListedOffline(target, artifacts, artifacts, symbols, { modules: o.roots })
          : await prepareRuntimePlanOffline(target, artifacts, artifacts, symbols);
        watchOutput = path.join(artifacts, 'writewatch.jsonl');
        // a plain image has no runtime areas to watch
        const env = o.plain ? {} : runtimeWatchEnv(prepared.planFile, watchOutput);
        record(artifacts, 'watch-environment', env);
        return {
          ...env,
          ...(o.relaxFs ? { DIGI_COLD_RELAX_FS: '1' } : {}),
          ...(typeof o.env === 'function' ? o.env(prepared) : o.env),
        };
      },
    },
    {
      prepareBaseline: async (session) => {
        if (o.startEmpty) {
          const startup = await session.status();
          record(artifacts, 'startup-empty', startup);
          expect(startup.firmware, 'the first baseline starts without firmware').toBeNull();
          expect(startup.cf, 'no ColdFire runtime exists before the first baseline').toBeUndefined();
        }
        // A cold daemon (no firmware at spawn) has no emulator, so no watch summary, for its first epoch.
        if (o.initial !== 'small' && o.dsp !== 'cold' && !o.startEmpty)
          epochs.add((await session.status()).epoch as number);
        if (o.initial === 'small') {
          image = path.join(freshDir(session), 'small-card.img');
          fs.writeFileSync(image, Buffer.alloc(1 << 20)); // blank; the identity, not the file, sets the capacity
        } else image = await build(session, 'initial', o.initial?.({ mod }));
        return baseline(image);
      },
      finishCase: async (runtime) => {
        const { session } = runtime;
        stepping?.dk.cancelPending();
        stepping = undefined;
        epochs.add(Number((await bounded(() => session.status())).epoch));
        overlays.push(await exportOverlay(session, `case-${++closed}`));
        await bounded(() => expectNoGuardHits(session));
        if (!o.faults) await bounded(() => session.expectNoFault(`case ${closed}`));
      },
      tempDirs: () => [artifacts],
      afterClose: async (_session) => {
        if (o.plain) return;
        const report = runtimeSafety(
          device,
          watchOutput,
          overlays,
          Math.max(1, epochs.size),
          'default',
          [...extra],
          [...cardServices],
        );
        record(artifacts, 'lifetime-safety', report);
        expect(report.write_events, 'write-watch on cf.expansion over every boot: no stray write').toBe(0);
        expect(report.flush_hits, 'flush check over every boot: no hit').toBe(0);
        // A blank small card is formatted by the stock firmware (writes below the store area): the test's own
        // "nothing written in the store range" check is the one that counts there, as in Python `run_card_small`.
        if (o.initial === 'small') {
          // no card-write policy
        } else if (o.uploads)
          for (const card of report.overlays)
            expect(
              card.outsideStore,
              `card writes of ${card.path}: the module store range and the firmware project sectors from 0x50000 only`,
            ).toEqual([]);
        else
          for (const card of report.overlays) {
            expect(
              card.invalid_store,
              'card writes in the store range: only the two selection sectors, the settings copies of a settings-writing service and the sectors the scenario writes on purpose',
            ).toEqual([]);
            expect(card.invalid_low, 'card writes elsewhere: only the firmware project sectors from 0x50000').toEqual(
              [],
            );
          }
        o.onClose?.(rig, report);
      },
    },
    scope,
  );

  const settle: CardRig['settle'] = async (
    runtime,
    afterEpoch = o.relaxFs && runtime.epoch ? runtime.epoch - 1 : undefined,
    options = {},
  ) => {
    if (o.plain) {
      await runtime.ui.wait({ ms: 300, why: SETTLE });
      await rpc(runtime.session, 'run.mode', { mode: 'paused' });
      return runtime;
    }
    if (o.hold) {
      // Python `settle_ms` (SETTLE_MS, 300 emulated ms), then `wait_dsp`: up to 5 s until no module is still LOADING (its DSP part follows the ColdFire part)
      await runtime.ui.wait({ ms: 300, why: SETTLE });
      await bootInfoLoaded(runtime.session, prepared.plan);
    }
    await expectRuntimeUi(runtime.session, prepared.plan, artifacts, {
      afterEpoch,
      paused: o.hold,
      emulatedMs: options.emulatedMs,
    });
    await rpc(runtime.session, 'run.mode', { mode: 'paused' });
    return runtime;
  };
  const afterCard = async (tag: string, base: string) => {
    const { session } = fixture.runtime;
    return cardAfter(base, await exportOverlay(session, tag, true), freshDir(session, 'after'));
  };
  const sectorsOf = (file: string) => Array.from(overlayRecords(fs.readFileSync(file)), (r) => r.sector);

  const rig: CardRig = {
    device,
    get plan() {
      return prepared.plan;
    },
    get artifacts() {
      return artifacts;
    },
    get runtime() {
      return fixture.runtime;
    },
    get session() {
      return fixture.runtime.session;
    },
    get cardImage() {
      return image;
    },
    get store() {
      return uploadStore(device);
    },
    get overlays() {
      return overlays;
    },
    get dk() {
      if (!stepping) throw new Error('no protocol session: boot with pausedBoot');
      return stepping.dk;
    },
    mod,
    cfg(name, bytes) {
      const file = path.join(artifacts, `cfg-${name}`);
      fs.writeFileSync(file, bytes);
      return { name: `cfg/${name}`, file };
    },
    card: (spec = {}) => build(fixture.runtime.session, spec.name ?? 'card', spec),
    async boot(cardImage, extraOptions = {}) {
      const next = await fixture.reload(baseline(cardImage, extraOptions.deliverExceptions));
      if (prepared.preparedImage) validateRuntimePlan(prepared.plan, next.session);
      epochs.add(next.epoch);
      image = cardImage;
      cap = undefined;
      return next;
    },
    async load(tag, spec) {
      const next = await rig.boot(await build(fixture.runtime.session, tag, spec));
      await settle(next);
      return next;
    },
    first: async () => (await settle(fixture.runtime)) as RuntimeCase,
    settle,
    async ready(safe = false) {
      const { session } = fixture.runtime;
      await pollEmulated(
        session,
        async () => {
          const b = await bootInfo(session, prepared.plan);
          return b.uiReached === 1 && b.safe === (safe ? 1 : 0);
        },
        Boolean,
        { emulatedMs: 300_000, intervalMs: 1000, message: `the UI loop was reached (safe mode: ${safe})` },
      );
      return bootInfo(session, prepared.plan);
    },
    async restart(runtime, { coreOnly = false } = {}) {
      const { session } = runtime;
      const before = Number((await bounded(() => session.status())).epoch);
      await rpc(session, 'run.mode', { mode: 'free' });
      await bounded(() => runtime.dk.restart());
      runtime.dk.cancelPending();
      const dk = emulatorDk(session);
      if (coreOnly) {
        // a core-only boot draws the reason frame and keeps the stock firmware: no runtime UI to settle on; the new boot is the new epoch
        cap = undefined;
        await pollEmulated(session, async () => Number((await session.status()).epoch) > before, Boolean, {
          emulatedMs: 120_000,
          intervalMs: 500,
          message: 'the restarted core-only boot was installed',
        });
        return withWaitApi({ session, dk, close: runtime.close });
      }
      // This controller can answer throughout the guest reset: settle proves the fresh ready epoch/UI hook.
      cap = undefined;
      // The reset boots to the boot window again and no daemon press answers it, so the rig answers it before its UI checks, as `Session.start` and `reloadRuntimeBaseline` do:
      // once the new boot has reached its main screen (the old boot's view stack stays in RAM until the reset clears it, so settling earlier could end on it).
      // A held rig keeps the daemon's press of NO (its baseline is paused: settling would step the emulator that its tests count from the ready point).
      if (!o.hold) {
        await pollEmulated(
          session,
          async () => {
            const status = await session.status();
            // the epoch changes when the new runtime is installed and the new runtime starts with main_ui_reached false (controller.rs install, common.rs is_ready), so the old boot's state cannot pass
            return typeof status.epoch === 'number' && status.epoch > before && status.cf?.main_ui_reached === true;
          },
          Boolean,
          { emulatedMs: 120_000, intervalMs: 500, message: 'the restarted boot reached its main screen' },
        );
        await session.settleBootWindow();
      }
      return settle(withWaitApi({ session, dk, close: runtime.close }), before);
    },
    usb: (runtime, run) => withPausedFxProtocol(runtime, run),
    after: (tag = `after-${++seq}`, base = image) => afterCard(tag, base),
    async records() {
      const { session } = fixture.runtime;
      return selRecords(session, image, device, await exportOverlay(session, `records-${++seq}`, true));
    },
    async storeWrites() {
      const { session } = fixture.runtime;
      return overlayWrites(await exportOverlay(session, `writes-${++seq}`), device).store_sectors.sort((a, b) => a - b);
    },
    async writes(tag) {
      return new Set(sectorsOf(await exportOverlay(fixture.runtime.session, tag)));
    },
    allowStoreWrites(sectors) {
      for (const s of sectors) extra.add(s);
    },
    async waitReset(maxSteps = WAIT_RESET_STEPS) {
      const { session } = fixture.runtime;
      const { client } = session;
      client.drainEvents();
      let resetSeen = false;
      // Emulated bound (steps that ran; a refused step ran nothing): 200 steps of 400k instructions are 80M, about 0.6 s of device time,
      // far above a reset after a fault. A stalled boot makes the no-progress watchdog close the session, and a closed connection ends the loop.
      let steps = 0;
      for (;;) {
        if (resetSeen) {
          // the controller notices that its worker has finished only when a command wakes it: a status request is one
          await delay(200);
          await session.status().catch((error) => {
            if (!(error instanceof RpcError)) throw error;
          });
        } else {
          if (steps >= maxSteps) throw new Error(`no reset within ${maxSteps * WAIT_RESET_STEP_INSNS} instructions`);
          await client.step(WAIT_RESET_STEP_INSNS, { timeout_wall_s: 30 }).then(
            () => void steps++,
            (error) => {
              // The daemon refuses steps during a cold boot; a lost connection is not refused, it is the end.
              if (!(error instanceof RpcError)) throw error;
              return delay(500);
            },
          );
        }
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
    },

    async pausedBoot(card) {
      const { session } = fixture.runtime;
      const previous = clone && path.dirname(clone);
      // Named after the card (path and overlay), so a path never maps to two cards across runs.
      const id = createHash('sha256')
        .update(`${fs.realpathSync(card)}:${fs.existsSync(overlayFile(card)) ? fileSha(overlayFile(card)) : ''}`)
        .digest('hex')
        .slice(0, 12);
      const next = path.join(session.runDir, `boot-${id}`, 'card.img');
      fs.mkdirSync(path.dirname(next), { recursive: true });
      linkCard(card, path.dirname(next), cardOverlay(card)); // the daemon may save next to the image: boot a card of its own
      const runtime = await rig.boot(next);
      await expectRuntimeUi(runtime.session, prepared.plan, artifacts, { afterEpoch: runtime.epoch - 1 });
      await drainInputs(runtime.session);
      await rpc(runtime.session, 'run.mode', { mode: 'paused' });
      await runtime.ui.wait({ ms: 300, why: SETTLE });
      if (previous && previous !== path.dirname(next)) fs.rmSync(previous, { recursive: true, force: true });
      clone = next;
      // The stepping transport and the stepped clock advance the paused emulator; waits count emulated ms.
      const transport = new EmuUsbTransport((method, params) => runtime.session.client.call(method, params), {
        stepInstructions: 400_000,
      });
      const clock = steppedClock(
        () => runtime.session.status(),
        (instructions) => runtime.session.client.call('step', { instructions }),
      );
      stepping = { dk: new DkSession(transport, { timeoutMs: 60_000, clock }), transport };
      return stepping.dk;
    },
    async view(tag) {
      return cardView(await afterCard(tag, image), device);
    },
    async ls(tag) {
      return storeList(await afterCard(tag, image), device);
    },
    async applyCap() {
      const { limit } = rig.store;
      if (limit * 512 > 0xffffffff) {
        await pokeKnob(fixture.runtime.session, prepared.plan, 'DKRT_STORE_CAP', CAP_SECTORS);
        cap = CAP_SECTORS;
      }
    },
    freeFresh: () => Math.min(((cap ?? rig.store.limit) - 2) * 512, 0xffffffff),
    freeWithPackage: () => rig.freeFresh() - systemPackageSectors() * 512,
    async request(cmd, body = new Uint8Array(), version) {
      if (!stepping) throw new Error('no protocol session: boot with pausedBoot');
      if (version === undefined) return stepping.dk.request(cmd, body);
      const seqNo = 0x55;
      await stepping.transport.send(frame(cmd, seqNo, body, version));
      for (;;) {
        const raw = await stepping.transport.recv(30_000);
        if (!raw) throw new Error('no reply to the raw request');
        const reply = parseReply(raw);
        if (reply && reply.seq === seqNo && reply.cmd === cmd) return reply;
      }
    },
    async status(cmd, body, version) {
      return (await rig.request(cmd, body, version)).status;
    },
    async send(cmd, body) {
      const status = await rig.status(cmd, body);
      if (status !== 0) throw new Error(`request ${cmd.toString(16)} answered ${status}`);
    },
  };
  return rig;
}
