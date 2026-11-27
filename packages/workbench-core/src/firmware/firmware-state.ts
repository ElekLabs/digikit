// The state of the Firmware page for the life of the window, and the actions on it (the page is FirmwarePage.tsx; the rules are in
// firmware-core.ts, the run in firmware-flow.ts). Signals, so the desktop app keeps a half-finished flash screen while another page is shown.
import { createSignal, untrack } from 'solid-js';
import { fmtVersion } from '@digi/dk-session';
import type { DeviceRecord } from '@digi/devices/vite';
import { registry } from '../data/device-profiles';
import { JobBusyError, type JobHandle } from '../jobs/jobs';
import { identify } from '../patch/identify';
import { sha256Hex } from '../runtime/local-inputs';
import { activateSystemPackage } from '../runtime/activate-system-package';
import type { TransferApp } from '../transfer/app';
import type { DefaultModulesApp } from './default-modules-state';
import type { FirmwareEnv } from './env';
import {
  IDLE,
  deviceNameOf,
  imageFileName,
  isDeviceId,
  leavesRuntime,
  modelOfPort,
  progressView,
  refusalText,
  running,
  type DeviceFacts,
  type DeviceState,
  type FlashRoute,
  type FlashState,
  type PrepareState,
} from './firmware-core';
import { runFlash, runRpcFlash } from './firmware-flow';
import { createFirmwareOps } from './firmware-ops';

const MAX_BYTES = 16 * 1024 * 1024;

export function createFirmwareState({
  env,
  transfer,
  defaults,
}: {
  env: FirmwareEnv;
  transfer: TransferApp;
  defaults: DefaultModulesApp;
}) {
  const { target } = transfer.target;
  const { asked, noteConnection } = transfer.connection;
  const { askDevice, portOps, rpcPortOps } = createFirmwareOps(env, transfer);
  const [prepared, setPrepared] = createSignal<PrepareState>({ kind: 'none' });
  const [restore, setRestoreSignal] = createSignal(false);
  const [device, setDevice] = createSignal<DeviceState>({ kind: 'checking' });
  const [ack, setAck] = createSignal(false);
  const [run, setRun] = createSignal<FlashState>(IDLE);
  const [notice, setNotice] = createSignal('');
  /** How the image gets to the device: the MidiRpc route (default), or the old OS UPGRADE menu route (a small fallback on the page). */
  const [route, setRouteSignal] = createSignal<FlashRoute>('rpc');

  function setRoute(next: FlashRoute): void {
    if (running(run())) return;
    setRouteSignal(next);
    setRun(IDLE);
    setAck(false);
  }

  let file: { name: string; bytes: Uint8Array; record: DeviceRecord | undefined } | undefined;
  let preparing = 0;
  /** Ticket of the newest file pick: a slow older pick must not overwrite a newer one. */
  let choosing = 0;
  /** The device record of the chosen file, from its sha256 against the registry; undefined when no file is chosen or patchkit has to decide. */
  const [fileDevice, setFileDevice] = createSignal<DeviceRecord | undefined>(undefined);

  /** Patch and check the picked file (again for another mode). A newer request makes the older answer unused. */
  async function prepare(): Promise<void> {
    if (!file) return;
    const mine = ++preparing;
    const { name, bytes, record } = file;
    if (record && !record.patchSets.device && !restore()) {
      setPrepared({
        kind: 'error',
        name,
        message: `There is no hardware image for the ${record.name} yet: its patched firmware runs only in the emulator. Restore stock still works.`,
      });
      return;
    }
    if (record && env.offersPatched && !env.offersPatched(record.id) && !restore()) {
      setPrepared({
        kind: 'error',
        name,
        message: `This site does not offer a patched image for the ${record.name} yet: the hardware route is not proven. Restore stock still works.`,
      });
      return;
    }
    setPrepared({ kind: 'working', name });
    try {
      // a device without a boot screen patch set gets the plain device set, whatever the stored toggle says
      const patchSet = record && !record.patchSets['device-introblank'] ? 'device' : defaults.patchSet();
      const p = await env.prepare(bytes, restore(), patchSet);
      if (mine !== preparing) return;
      setPrepared({ kind: 'ready', name, size: bytes.length, p });
    } catch (e) {
      if (mine !== preparing) return;
      setPrepared({ kind: 'error', name, message: refusalText(e instanceof Error ? e.message : String(e)) });
    }
  }

  /** The user picked (or dropped) a file. */
  async function chooseFile(picked: File | undefined): Promise<void> {
    if (!picked || running(run())) return;
    const mine = ++choosing;
    setRun(IDLE);
    setAck(false);
    if (picked.size > MAX_BYTES) {
      file = undefined;
      setFileDevice(undefined);
      setPrepared({
        kind: 'error',
        name: picked.name,
        message: 'That file is too large for a firmware update (the limit is 16 MB).',
      });
      return;
    }
    const bytes = new Uint8Array(await picked.arrayBuffer());
    if (mine !== choosing) return;
    const hash = await sha256Hex(bytes);
    if (mine !== choosing) return;
    const id = identify(hash)?.device;
    const record = id === undefined ? undefined : registry.get(id);
    file = { name: picked.name, bytes, record };
    setFileDevice(record);
    await prepare();
  }

  function setRestore(on: boolean): void {
    if (running(run())) return;
    setRestoreSignal(on);
    setRun(IDLE);
    void prepare();
  }

  /** The Custom boot screen toggle: the patch gets core/introblank (or not) and the file is prepared again. */
  function setCustomBoot(on: boolean): void {
    if (running(run())) return;
    defaults.setCustomBootScreen(on);
    setRun(IDLE);
    void prepare();
  }

  /** Look at the selected port: the model, the OS version, the runtime. Feeds the connection indicator too. */
  async function probeDevice(): Promise<void> {
    if (running(run())) return;
    const t = target();
    if (t.kind === 'emulator' && env.emulatorFlash) {
      if (device().kind !== 'connected') setDevice({ kind: 'checking' });
      try {
        const result = await env.emulatorFlash.probe(transfer.target.bindSession());
        if (!running(run()) && target() === t) setDevice(result);
      } catch (e) {
        if (!running(run()) && target() === t)
          setDevice({ kind: 'none', why: e instanceof Error ? e.message : String(e) });
      }
      return;
    }
    if (t.kind !== 'midi') {
      const why =
        t.kind === 'emulator'
          ? 'The emulator is not flashed from this page. Pick the MIDI port of a device under Target.'
          : ((await transfer.env.targets.whyNone?.()) ??
            'No MIDI port. Connect the device by USB, set its USB mode to MIDI, then pick it under Target.');
      setDevice({ kind: 'none', why });
      noteConnection({ type: 'lost', reason: why });
      return;
    }
    if (device().kind !== 'connected') setDevice({ kind: 'checking' });
    try {
      const a = await asked(() => askDevice());
      if (running(run()) || target().kind !== 'midi') return;
      const model = isDeviceId(a.model ?? '') ? a.model : modelOfPort(a.port);
      const facts: DeviceFacts = {
        port: a.port,
        model,
        ...(a.os ? { os: a.os } : {}),
        ...(a.runtime ? { runtime: a.runtime } : {}),
        ...(a.imageId !== undefined ? { imageId: a.imageId } : {}),
      };
      setDevice({ kind: 'connected', d: facts });
      noteConnection({
        type: 'answered',
        device: {
          id: model ?? '',
          name: model ? deviceNameOf(model) : 'MIDI device',
          ...(a.os ? { os: a.os } : {}),
          ...(a.runtime ? { runtime: a.runtime } : {}),
        },
      });
    } catch (e) {
      if (running(run())) return;
      setDevice({ kind: 'lost', reason: e instanceof Error ? e.message.split('\n')[0]! : String(e) });
    }
  }

  /** The target changed: the old reading is not shown. */
  function forgetDevice(): void {
    setDevice({ kind: 'checking' });
    setAck(false);
  }

  let controller: AbortController | undefined;

  /** Send the image. The caller has checked `canFlash`. */
  async function startFlash(): Promise<void> {
    const prep = prepared();
    const t = target();
    const emulator = t.kind === 'emulator' ? env.emulatorFlash : undefined;
    if (prep.kind !== 'ready' || (t.kind !== 'midi' && !emulator) || running(run())) return;
    const p = prep.p;
    const log = env.log;
    const what = p.restore ? 'restore' : 'flash';
    const flashRoute = route();
    const fileChoice = choosing;
    const rpc = !!emulator || flashRoute === 'rpc';
    const destination = t.kind === 'midi' ? t.port : 'Emulator';
    let final: FlashState = IDLE;
    try {
      const work = async (job: JobHandle, targetJob?: JobHandle): Promise<FlashState> => {
        controller = new AbortController();
        const { signal } = controller;
        const initialBinding = transfer.target.bindSession();
        let committed = false;
        const startedMs = performance.now();
        if (emulator)
          setRun({ phase: 'sending', sent: 0, total: p.image.length, startedMs, nowMs: startedMs, unit: 'bytes' });
        log({
          level: 'info',
          text: `${what} ${p.device} OS ${p.os} to ${destination}: ${rpc ? `${p.image.length} bytes (MidiRpc)` : `${p.ends.length} messages (OS UPGRADE menu)`}`,
        });
        setNotice('');
        defaults.resetPostRun();
        const report = (s: FlashState): void => {
          if (emulator && s.phase === 'rebooting' && !committed) {
            committed = true;
            controller = undefined;
            job.disableCancel?.();
            targetJob?.disableCancel?.();
          }
          if (s.phase === 'done' && (emulator || env.systemPackage) && leavesRuntime(p)) return;
          setRun(s);
          if (s.phase === 'sending') {
            const v = progressView(s);
            job.progress(v.fraction, v.text);
          } else if (s.phase === 'rebooting' && s.stage === 'asking' && s.setup) {
            job.progress(s.setup.fraction, s.setup.text);
          } else job.progress(undefined, s.phase === 'rebooting' ? 'restarting' : undefined);
        };
        try {
          const pkg = leavesRuntime(p) ? await env.systemPackage?.() : undefined;
          initialBinding.check();
          signal.throwIfAborted();
          if (prepared() !== prep || route() !== flashRoute || choosing !== fileChoice)
            throw new Error('The firmware or flash route changed. Review your selection and try again.');
          if (pkg && fmtVersion(pkg.runtime) !== p.runtime)
            throw new Error('The Workbench system package version differs from the prepared firmware.');
          if (emulator) {
            const result = await emulator.flash(initialBinding, p, signal, report);
            const flashed = result.state;
            if (flashed.phase === 'done' && leavesRuntime(p)) {
              const sinceMs = performance.now();
              const progress = (fraction: number | undefined, text: string) =>
                report({
                  phase: 'rebooting',
                  rpc: true,
                  stage: 'asking',
                  sinceMs,
                  nowMs: performance.now(),
                  setup: { fraction, text },
                });
              try {
                initialBinding.check();
                if (pkg) {
                  progress(undefined, 'Checking system package');
                  const changed = await activateSystemPackage(await initialBinding.session(), pkg, {
                    identity: { device: p.device, imageId: p.imageId },
                    check: initialBinding.check,
                    beforeRestart: result.beforePackageRestart,
                    progress,
                  });
                  flashed.lines.push(
                    changed ? 'System package verified and active.' : 'System package already active.',
                  );
                }
                await finishRuntime(p.device, 'emulator', initialBinding.check, progress);
              } catch (e) {
                notePackageFailure(flashed, e instanceof Error ? e.message : String(e));
              }
            }
            setRun(flashed);
            return flashed;
          }
          const ops = rpc ? rpcPortOps() : portOps();
          let flashed: FlashState;
          try {
            flashed =
              'machine' in ops ? await runRpcFlash(ops, p, signal, report) : await runFlash(ops, p, signal, report);
          } finally {
            transfer.target.finishRawTransport();
          }
          if (flashed.phase !== 'done' || !pkg) return flashed;
          let packageReady = false;
          try {
            const binding = transfer.target.bindSession();
            if (binding.target.kind !== 'midi' || binding.target.port !== ops.port)
              throw new Error('The selected target changed.');
            const changed = await activateSystemPackage(await binding.session(), pkg, {
              identity: { device: p.device, imageId: p.imageId },
              check: binding.check,
              signal,
              progress: job.progress,
            });
            flashed.lines.push(changed ? 'System package verified and active.' : 'System package already active.');
            packageReady = true;
          } catch (e) {
            const detail = signal.aborted
              ? 'Setup was stopped. The package may already be on the card.'
              : e instanceof Error
                ? e.message
                : String(e);
            notePackageFailure(flashed, detail);
          }
          setRun(flashed);
          if (packageReady) await finishRuntime(p.device);
          return flashed;
        } catch (e) {
          if (emulator && !committed && signal.aborted) {
            const stopped: FlashState = {
              phase: 'stopped',
              reason: 'cancelled',
              sent: 0,
              total: p.image.length,
              unit: 'bytes',
              message: 'Cancelled before flashing.',
            };
            setRun(stopped);
            return stopped;
          }
          const failed: FlashState = {
            phase: 'failed',
            code: 'send',
            message: e instanceof Error ? e.message : String(e),
          };
          setRun(failed);
          return failed;
        } finally {
          controller = undefined;
        }
      };
      const label = emulator && p.restore ? 'Restoring stock firmware' : 'Flashing firmware';
      const lockedTarget = (emulatorJob?: JobHandle) =>
        transfer.jobs.runJob(label, { group: 'target', cancel: cancelFlash }, (job) =>
          untrack(() => work(emulatorJob ?? job, emulatorJob && job)),
        );
      final = emulator
        ? await transfer.jobs.runJob(label, { group: 'emulator', cancel: cancelFlash }, lockedTarget)
        : await lockedTarget();
    } catch (e) {
      if (!(e instanceof JobBusyError)) throw e;
      final = { phase: 'failed', code: 'send', message: 'a transfer is running; wait for it' };
      setRun(final);
    }
    setAck(false);
    // the device is another device now (or the same one, restarted): read it again
    void probeDevice();
    if (final.phase === 'done')
      log({ level: 'info', text: `${what} done: ${final.message}`, detail: final.lines.join('\n') });
    else if (final.phase === 'stopped')
      log({
        level: final.reason === 'error' ? 'error' : 'info',
        text: `${what} stopped after ${final.sent} of ${final.total} ${final.unit ?? 'messages'}`,
        detail: final.message,
      });
    else if (final.phase === 'failed') log({ level: 'error', text: `${what} failed: ${final.message}` });
    if (!emulator && final.phase === 'done' && leavesRuntime(p) && !env.systemPackage) await finishRuntime(p.device);
  }

  function notePackageFailure(flashed: Extract<FlashState, { phase: 'done' }>, detail: string): void {
    const message = `System package setup failed. ${detail} If the target is still core-only, choose Transfer → Modules → Install system package. Otherwise check the device and restart it.`;
    flashed.lines.push(message);
    env.log({ level: 'error', text: message });
  }

  /** Replace the runtime's services and install the selected defaults after the device is ready. */
  async function finishRuntime(
    deviceId: string,
    where: 'device' | 'emulator' = 'device',
    check?: () => void,
    progress?: (fraction: number | undefined, text: string) => void,
  ): Promise<void> {
    check?.();
    progress?.(undefined, 'Refreshing services on the emulator card');
    try {
      const replaced = await transfer.backend.refreshServices(deviceId, () => {});
      if (replaced.length) env.log({ level: 'info', text: `services replaced: ${replaced.join(', ')}` });
    } catch (e) {
      env.log({
        level: 'error',
        text: `Services: ${e instanceof Error ? e.message : String(e)}. The firmware flash is done; Install on the Transfer page replaces them.`,
      });
    }
    check?.();
    await defaults.installAfterFlash(deviceId, where, ({ done, total, text }) => progress?.(done / total, text));
  }

  function cancelFlash(): void {
    controller?.abort();
  }

  /** Back to the start after a stopped or finished run. */
  function resetRun(): void {
    if (running(run()) || defaults.postWorking()) return;
    setRun(IDLE);
    defaults.resetPostRun();
  }

  /** Give the user the image as a file. */
  async function downloadImage(): Promise<void> {
    const prep = prepared();
    if (prep.kind !== 'ready') return;
    try {
      const where = await env.saveImage(imageFileName(prep.p), prep.p.image);
      setNotice(where ? `Saved ${where}` : '');
      if (where) env.log({ level: 'info', text: `saved the image: ${where}` });
    } catch (e) {
      setNotice(`Could not save the file: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  /** The site: open the patched image in the emulator page. */
  async function runInEmulator(): Promise<void> {
    const prep = prepared();
    const open = env.runInEmulator;
    if (prep.kind !== 'ready' || !open) return;
    try {
      await open(imageFileName(prep.p), prep.p.device, prep.p.image);
    } catch (e) {
      setNotice(`Could not pass the file to the emulator: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  return {
    prepared,
    fileDevice,
    setPrepared,
    restore,
    setRestore,
    device,
    ack,
    setAck,
    run,
    notice,
    route,
    setRoute,
    chooseFile,
    setCustomBoot,
    probeDevice,
    forgetDevice,
    startFlash,
    cancelFlash,
    resetRun,
    downloadImage,
    runInEmulator,
  };
}
export type FirmwareState = ReturnType<typeof createFirmwareState>;
