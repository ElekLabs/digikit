import { expect, test } from 'vitest';
import { SETTLE } from '@digi/regression/runtime/panel-waits.ts';
import { ufSuite, PAIR } from './usb-fast-support.ts';

/** The firmware's USB mode switch and the module's timer: no bulk session in USB AUDIO/MIDI until a nonce, the bulk pair after the switch to USB MIDI. */

ufSuite('USB mode', ({ rig, baseCard, connect, setPort }) => {
  test('a device in USB AUDIO/MIDI holds no bulk session until a nonce; the switch to USB MIDI brings the bulk pair up on a timer of the module', async () => {
    // The harness boots in USB MIDI, so the boot's enable happens first (the job's first step runs at init); the switch to port 5 then ends that session.
    const dk = await rig.pausedBoot(await baseCard());
    await rig.runtime.ui.wait({ ms: 2500, why: SETTLE });
    await setPort(5);
    await rig.runtime.ui.wait({ ms: 3000, why: SETTLE });
    const status = async () => {
      const b = Buffer.from((await dk.request(0x37)).body);
      return { enabled: (b.readUInt32BE(28) & 4) !== 0, ring: b.readUInt32BE(76) };
    };
    expect(await status(), 'USB AUDIO/MIDI without a nonce: no session, no ring').toEqual({ enabled: false, ring: 0 });
    await setPort(2);
    await rig.runtime.ui.wait({ ms: 1500, why: SETTLE });
    expect((await status()).enabled, 'USB MIDI again: the job enabled the bulk session').toBe(true);
    const bulk = await connect(dk, { mode: PAIR });
    expect((await bulk.session.info()).flags & 1, 'the pair answers INFO on its own endpoint').toBe(0);
    await rig.session.expectNoFault('usb-fast mode switch');
  }, 1_800_000);
});
