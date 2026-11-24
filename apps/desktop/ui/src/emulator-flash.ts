import { poll, wallClock } from '@digi/dk-session';
import type { EmulatorFlash, FirmwareEnv } from '@digi/workbench-core/firmware/env.ts';
import { isDeviceId } from '@digi/workbench-core/firmware/firmware-core.ts';
import { driveRpcFlash } from '@digi/workbench-core/firmware/firmware-flow.ts';
import { EmuUsbTransport } from '@digi/workbench-core/host/emu-transport.ts';
import type { EmuRpc } from '@digi/workbench-core/host/host.ts';
import type { ControllerJob } from '@digi/workbench-core/jobs/jobs-core.ts';
import type { PreparedFlash } from '@digi/workbench-core/patch/patchkit-core.ts';
import type { FirmwareInfo, RuntimePatch } from '@digi/workbench-core/runtime/runtime.ts';
import { sha256Hex } from '@digi/workbench-core/runtime/local-inputs.ts';
import { PAUSED } from './desktop-env';

interface FlashStatus {
  session: number;
  epoch: number;
  mode: string;
  job: ControllerJob | null;
  cf?: { device: string; version: string; ready: boolean };
  firmware: { base_sha256: string; syx_sha256: string; runtime?: RuntimePatch | null } | null;
  dsp_enabled?: boolean;
  dsp_flag?: boolean | null;
  flash_boot?: { from_epoch: number; container_sha256: string; syx_sha256: string } | null;
}

const digest = /^[a-f0-9]{64}$/;
const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** Controller status and the selected emulator's USB session are distinct sources of evidence. */
export function createEmulatorFlash(
  rpc: EmuRpc,
  machine: FirmwareEnv['rpc'],
  ui: { paused(): boolean; refreshAfterFlash(info: FirmwareInfo): void },
): EmulatorFlash {
  async function status(): Promise<FlashStatus> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      return await Promise.race([
        rpc.call<FlashStatus>('status'),
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(new Error('The emulator controller did not answer.')), 5000);
        }),
      ]);
    } finally {
      clearTimeout(timer);
    }
  }

  const waitForStatus = (done: (s: FlashStatus) => boolean, message: string) =>
    poll(
      wallClock,
      () => status().catch(() => undefined),
      (s) => !!s && done(s),
      {
        withinMs: 180_000,
        intervalMs: 250,
        message,
      },
    );

  function problem(s: FlashStatus): string | undefined {
    if (ui.paused()) return PAUSED;
    if (s.mode === 'paused')
      return 'The emulator controller is paused. Resume it through emulator control or reopen the emulator in normal mode.';
    if (s.job) return 'The emulator is busy. Wait for its current job to finish.';
    if (!s.firmware || !s.cf) return 'Load stock firmware on the Emulator page, then try again.';
    if (!s.cf.ready) return 'Wait for the emulator to finish booting, then try again.';
    if (s.mode !== 'free') return 'Resume the emulator in free-running mode, then try again.';
    if (s.flash_boot === undefined) return 'The emulator controller does not report flash verification.';
    return undefined;
  }

  function checkPrepared(s: FlashStatus, p: PreparedFlash): void {
    const refusal = problem(s);
    if (refusal) throw new Error(refusal);
    if (s.cf?.device !== p.device) throw new Error('The image is for a different device than the loaded emulator.');
    if (s.firmware?.base_sha256 !== p.shaStock)
      throw new Error('Load the same stock firmware on the Emulator page before flashing this image.');
  }

  return {
    async probe(binding) {
      if (binding.target.kind !== 'emulator') throw new Error('The selected target changed.');
      const s = await status();
      binding.check();
      const why = problem(s);
      if (why) return { kind: 'none', why };
      const device = s.cf!;
      return {
        kind: 'connected',
        d: {
          port: 'Emulator',
          model: isDeviceId(device.device) ? device.device : undefined,
          os: device.version,
          baseSha256: s.firmware!.base_sha256,
        },
      };
    },
    async flash(binding, p, signal, report) {
      if (binding.target.kind !== 'emulator') throw new Error('The selected target changed.');
      binding.check();
      signal.throwIfAborted();
      const expected = p.containerSha256;
      if (!p.ok || !expected || !digest.test(expected) || (await sha256Hex(p.image)) !== p.shaImage)
        throw new Error('The prepared firmware has no verified container digest.');
      binding.check();
      const readPrepared = async () => {
        const s = await status();
        binding.check();
        signal.throwIfAborted();
        checkPrepared(s, p);
        return s;
      };
      await readPrepared();

      const release = await (await binding.session()).reserveTransport();
      let origin: FlashStatus;
      const transport = new EmuUsbTransport({
        call: (method, params) =>
          method === 'midi.usb.in'
            ? rpc.call(method, { ...params, epoch: origin.epoch }, origin.session)
            : rpc.call(method, params),
      });
      const started = performance.now();
      const rebooting = (stage: 'leaving' | 'returning') =>
        report({ phase: 'rebooting', rpc: true, stage, sinceMs: started, nowMs: performance.now() });
      let flashed: FlashStatus | undefined;
      let state: Awaited<ReturnType<typeof driveRpcFlash>>;
      try {
        state = await driveRpcFlash(
          {
            now: () => performance.now(),
            machine: async (image, chunk) => {
              const m = await machine(image, chunk);
              try {
                // Start capture and drain the controller's pending output while the session is reserved.
                binding.check();
                signal.throwIfAborted();
                await rpc.call('midi.usb.out');
                origin = await readPrepared();
                return m;
              } catch (error) {
                m.free();
                throw error;
              }
            },
            send: (msg) => transport.send(msg),
            recv: (ms, cancel) => transport.recv(ms, cancel),
          },
          p,
          signal,
          report,
          async () => {
            try {
              flashed = await waitForStatus((s) => {
                if (s.session !== origin.session) throw new Error('The emulator session changed during the flash.');
                if (s.job || s.epoch > origin.epoch) rebooting('returning');
                const receipt = s.flash_boot;
                if (receipt && receipt.from_epoch >= origin.epoch) {
                  if (
                    receipt.from_epoch !== origin.epoch ||
                    receipt.container_sha256 !== expected ||
                    !digest.test(receipt.syx_sha256) ||
                    receipt.syx_sha256 !== s.firmware?.syx_sha256 ||
                    s.firmware.base_sha256 !== p.shaStock
                  )
                    throw new Error('The emulator booted a different firmware container.');
                  if (s.epoch > origin.epoch && !s.job && s.cf?.ready && s.cf.device === p.device) return true;
                }
                if (s.mode === 'paused' && !s.job) throw new Error('The emulator stopped during the flash reboot.');
                return false;
              }, 'Could not verify a new emulator boot from the prepared flash container');
            } catch (error) {
              const failure = { phase: 'failed', code: 'no-restart', message: errorText(error) } as const;
              report(failure);
              return failure;
            }
            return {
              phase: 'done' as const,
              message: p.restore
                ? 'The emulator rebooted into stock firmware.'
                : 'The emulator booted the flashed firmware.',
              lines: [],
            };
          },
          0,
          {
            commit: () => rebooting('leaving'),
            failure: (code, message) => {
              if (code === 'timeout' && message.includes('Check the cable'))
                return 'The emulator did not answer the firmware request. Check that it is running and try again.';
              if (code === 'crc')
                return 'The emulator could not verify the firmware data. Try again. Nothing was written.';
              return message;
            },
          },
        );
      } finally {
        release();
      }
      if (!flashed) return { state };

      ui.refreshAfterFlash({
        runtime: flashed.firmware?.runtime ?? null,
        ...(flashed.dsp_enabled === undefined
          ? {}
          : { dsp: { enabled: flashed.dsp_enabled, flag: flashed.dsp_flag ?? null } }),
      });
      const verified = flashed;
      const beforePackageRestart = async () => {
        binding.check();
        const s = await status();
        binding.check();
        const sameImage = (current: FlashStatus) =>
          current.session === verified.session && current.firmware?.syx_sha256 === verified.firmware?.syx_sha256;
        if (!sameImage(s) || s.job || !s.cf?.ready) throw new Error('The emulator firmware changed.');
        return async () => {
          await waitForStatus((current) => {
            binding.check();
            if (current.session !== s.session || (current.firmware && !sameImage(current)))
              throw new Error('The emulator firmware changed.');
            return current.epoch > s.epoch && sameImage(current) && !current.job && current.cf?.ready === true;
          }, 'Could not confirm the emulator restarted for the system package');
        };
      };
      return { state, beforePackageRestart };
    },
  };
}
