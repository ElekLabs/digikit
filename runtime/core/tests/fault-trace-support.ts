import { CAP, SERVICE_CMD } from '@digi/dk-session/gen_proto';
import { expect } from 'vitest';
import type { PatchedSession } from '@digi/regression/support.ts';
import { BOOT_FIELDS } from '@digi/regression/runtime/ready.ts';
import { readSmallRing } from '@digi/regression/runtime/runtime-menu.ts';
import { planSymbol, prepareRuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { pollEmulated, withEmulatedProbe } from '@digi/regression/runtime/waits.ts';

/** The helpers of the fault-trace tests that wait for the safe boot after a stop or a crash (the two `fault-trace` device test files). */

/** Wait for the device to restart itself into safe mode (the hang record is written about 20 s after the DSP stopped). */
export async function waitForSafeMode({ session, dk }: PatchedSession): Promise<void> {
  await pollEmulated(session, async () => (await withEmulatedProbe(session, (q) => dk.info(q))).safe_mode, Boolean, {
    emulatedMs: 180_000,
    intervalMs: 2000,
    retry: true,
    message: 'safe mode after the stop',
  });
}

/** Wait until the safe boot has reached the UI loop: DKRT_BOOT has `safe` set and `ui_reached` raised (the first UI pass). The main OS starts the UI
 * loop after the boot intro, and the boot hook has already kept `dk/fault` and the log events by then. The wait is in emulated time. */
export async function waitForSafeUi({ session }: PatchedSession, plan: Awaited<ReturnType<typeof prepareRuntimePlan>>): Promise<void> {
  const base = planSymbol(plan, 'DKRT_BOOT');
  await pollEmulated(
    session,
    async () => {
      const [safe, ui] = await Promise.all([
        session.client.memPeek(base + BOOT_FIELDS.safe),
        session.client.memPeek(base + BOOT_FIELDS.ui_reached),
      ]);
      return safe === 1 && ui === 1;
    },
    Boolean,
    { emulatedMs: 180_000, intervalMs: 1000, retry: true, message: 'the safe boot reached the UI loop' },
  );
}

/** After a stop or a crash the boot is safe mode: INFO has no LOG capability (the debug module is not loaded). Returns the runtime's own ring. */
export async function expectNoDebugModule(runtime: PatchedSession, plan: Awaited<ReturnType<typeof prepareRuntimePlan>>) {
  const { session, dk } = runtime;
  const info = await withEmulatedProbe(session, (q) => dk.info(q));
  expect(info.safe_mode).toBe(true);
  expect(info.caps !== null && (info.caps >>> CAP.LOG) & 1, 'no LOG capability without the debug module').toBe(0);
  expect((await withEmulatedProbe(session, (q) => dk.request(SERVICE_CMD.LOG, Uint8Array.of(0, 0, 0, 0), q))).status).toBe(-60);
  return readSmallRing(session, plan);
}
