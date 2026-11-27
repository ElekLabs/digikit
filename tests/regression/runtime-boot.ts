import { expect } from 'vitest';
import { pollEmulated } from './runtime/waits.ts';
import type { PatchedSession } from './test-runtime.ts';
import { timingContext, timePhase } from './timing.ts';

/** Boot identity and health only: runtime patch sets do not install remote-control modules. */
export async function expectRuntimeBoot({ session, dk }: PatchedSession): Promise<void> {
  await timePhase(
    'boot.validation',
    async () => {
      const identity = session.loadResult.runtime;
      const prepared = session.preparedRuntimeImage;
      if (prepared) {
        expect(session.loadResult.syx_sha256).toBe(prepared.syxSha256);
        expect(prepared.modules.length).toBeGreaterThan(0);
      } else {
        expect(identity?.patched).toBe(true);
        if (!identity?.patched) throw new Error('stock input did not produce a runtime patch identity');
        expect(identity.set).toBe(session.options.runtime?.patchSet ?? 'emulator');
        expect(identity.base_sha256).toMatch(/^[a-f0-9]{64}$/);
        expect(identity.modules.length).toBeGreaterThan(0);
        expect(identity.cache_file).toBeTruthy();
      }
      expect(session.loadResult.start_used).toBeTruthy();
      await pollEmulated(session, async () => (await session.status()).cf.ready, Boolean, {
        emulatedMs: 20_000,
        message: 'the ColdFire runtime is ready',
      });
      const status = await session.status();
      expect(status.mode).toBe('free');
      expect(status.cf.ready).toBe(true);
      if (prepared) {
        expect(status.firmware?.syx_sha256).toBe(prepared.syxSha256);
        expect(status.firmware?.base_sha256).toBe(prepared.baseSha256);
      } else expect(status.firmware?.runtime).toEqual(identity);
      expect(dk.rpcChunk).toBe(16384);
      await session.expectNoFault('runtime boot');
    },
    timingContext(session),
  );
}
