import { RANGES } from '@digi/dk-session/gen_proto';
import { expect } from 'vitest';
import type { PatchedSession } from '@digi/regression/support.ts';
import { installModule, moduleArtifact } from '@digi/regression/runtime/install.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';
import { emulatorClock, waitClock } from '@digi/test-support';
import { pollEmulated } from '@digi/regression/runtime/waits.ts';

/**
 * The entry point `dk_module` and the service imports (docs/plans/sdk.md section 5; the fixture test/entry-demo uses only the new contract).
 * `init` gets a `dk_module_ctx` and opens the settings; the optional USB bulk service is not in the image, so its `_open` answers DK_E_MISSING and the module
 * still loads; `tick` runs once per UI pass; a settings write queued by a host command runs in `tick`.
 */
const REPORT = 0x1c;
const WRITE = 0x1d;
const E_MISSING = -73;

async function expectEntryPoint({ session, dk }: PatchedSession): Promise<void> {
  expect(['zero', 'none'], 'the entry point needs no DSP').toContain((await session.status()).dsp);
  await expectRuntimeReady(session);
  await installModule(dk, 'test/entry-demo', moduleArtifact(session.options.device, 'test/entry-demo'), {
    device: session.options.device,
  });
  expect(RANGES.entry_demo_test, 'reserved command range of the fixture').toEqual([0x1c, 0x1d]);
  const report = async () => {
    const reply = await dk.request(REPORT);
    expect(reply.status, 'the module registered its commands in init').toBe(0);
    const b = Buffer.from(reply.body);
    expect(b.length, 'report length').toBe(42);
    return {
      settingsOpen: b.readInt32BE(0),
      bootRead: b.readInt32BE(4),
      usbOpen: b.readInt32BE(8),
      usbHandleNull: b.readUInt32BE(12),
      device: b.readUInt32BE(16),
      runtime: b.readUInt32BE(20),
      ticks: b.readUInt32BE(24),
      tickSpan: b.readUInt32BE(28),
      put: b.readInt32BE(32),
      back: b.readInt32BE(36),
      value: b[40]!,
      pending: b[41]!,
    };
  };

  const first = await report();
  expect(first.usbOpen, 'dk_usb_bulk_open without core/usb-bulk is DK_E_MISSING').toBe(E_MISSING);
  expect(first.usbHandleNull, 'the absent service leaves the handle NULL').toBe(1);

  // The UI loop (and with it `tick`) starts some emulated seconds after the boot finished and the module registered: wait for the first tick (firmware
  // state), then let a second of emulated time pass.
  const running = await pollEmulated(session, report, (r) => r.ticks > 0, {
    emulatedMs: 30_000,
    message: 'the UI loop never called tick',
  });
  await waitClock(emulatorClock(session), 1000); // a second of ticks: no observable condition beyond the first tick
  const later = await report();
  expect(later.ticks, 'tick runs on every UI pass').toBeGreaterThan(running.ticks);
  expect(later.tickSpan, 'dk_time_blocks advances between the first and the last tick').toBeGreaterThan(0);

  expect((await dk.request(WRITE, Uint8Array.of(0x5a))).status, 'the write is queued').toBe(0);
  await waitClock(emulatorClock(session), 500); // the tick that runs the queued write has no event
  const written = await report();
  expect(written.pending, 'tick ran the queued write').toBe(0);
  expect([written.put, written.back, written.value], 'dk_settings_put, then dk_settings_get in tick').toEqual([0, 1, 0x5a]);
  await session.expectNoFault('entry point');
}

runtimeTests('a module of the new entry point loads, ticks and finds an optional service absent', expectEntryPoint, {
  dsp: 'zero',
});
