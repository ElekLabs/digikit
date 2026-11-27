import { expect, test } from 'vitest';
import { ufSuite, SAMPLES, STORE, pattern, hashU32 } from './usb-fast-support.ts';

/** Throughput in emulated time, bulk against SysEx, on the full tier (`DIGIKIT_TIER=full`): the emulator is not a speed reference, so the test only checks the model's ordering. */

ufSuite(
  'bench',
  ({ device, dt2, rp, connect, boot, elapsed }) => {
    test('bench: MB/s in emulated time, bulk against SysEx', async () => {
      const { dk } = await boot();
      const size = 1 << 20;
      const data = pattern(size, 55);
      const fast = await connect(dk, { stepInstructions: 20_000 });
      const clock = async (run: () => Promise<unknown>) => {
        const a = await elapsed();
        await run();
        return ((await elapsed()) - a) / 132e6; // seconds of emulated time at 132 MHz
      };
      const lines: string[] = [];
      const tBulk = await clock(() =>
        fast.session.put(STORE, 'data/bench-bulk.bin', data, { expected: hashU32(data, false) }),
      );
      lines.push(`bulk to the store: ${(size / 1e6 / tBulk).toFixed(2)} MB/s`);
      // the same upload with the receive ring off (the old 8 slots and window 2): a sanity check only, the emulator is not a speed reference
      await dk.request(0x37, Uint8Array.from([0x53, 1, 0]));
      const old = await connect(dk, { stepInstructions: 20_000 });
      const tOld = await clock(() =>
        old.session.put(STORE, 'data/bench-bulk-old.bin', data, { expected: hashU32(data, false) }),
      );
      lines.push(`bulk to the store, ring off: ${(size / 1e6 / tOld).toFixed(2)} MB/s`);
      await dk.request(0x37, Uint8Array.from([0x53, 1, 1]));
      const tSys = await clock(() => dk.put('data/bench-sysex.bin', data));
      lines.push(`SysEx to the store: ${(size / 1e6 / tSys).toFixed(2)} MB/s`);
      if (dt2) {
        const tDrive = await clock(() =>
          fast.session.put(SAMPLES, rp('bench.bin'), data, { expected: hashU32(data, true) }),
        );
        lines.push(`bulk to the +Drive: ${(size / 1e6 / tDrive).toFixed(2)} MB/s`);
      }
      // eslint-disable-next-line no-console
      console.log(`BENCH ${device.toUpperCase()} 1 MiB, emulated time (132 MHz model): ${lines.join('; ')}`);
      expect(tBulk, 'bulk is faster than SysEx in the model').toBeLessThan(tSys);
    }, 3_600_000);
  },
  { full: true },
);
