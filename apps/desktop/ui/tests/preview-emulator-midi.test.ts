// The preview's Emulator target answers SysEx through `midi.usb.in` / `midi.usb.out` with the responder of the MIDI targets.
import { expect, it, vi } from 'vitest';
import { decode8in7, encode8in7 } from '@digi/dk-session';
import { CAP, CMD, SERVICE_CMD } from '@digi/dk-session/gen_proto';
import { deviceProfiles, runtimeDeviceIds } from '@digi/workbench-core/data/device-profiles.ts';

const dev = runtimeDeviceIds[0]!;
const fixture = {
  dev,
  state: 'runtime',
  os: deviceProfiles[dev].firmware.at(-1)!.version,
  flashMs: 0,
  goneMs: 0,
  sent: 0,
};
const DEBUG_CAPS = (1 << CAP.LOG) | (1 << CAP.PEEK) | (1 << CAP.LOGCFG);

async function emulator(modules: string[]) {
  vi.stubGlobal('location', { search: '' }); // the preview's sim reads the page query
  const { createFirmwareMidi } = await import('../preview/firmware-midi');
  const midi = createFirmwareMidi(new URLSearchParams(), fixture);
  midi.readCard(async () => modules);
  const ask = async (cmd: number, seq: number, body: number[] = []) => {
    midi.usbIn(dev, Uint8Array.of(0xf0, 0x7d, 0x64, 0x6b, 1, cmd, seq, ...encode8in7(Uint8Array.from(body)), 0xf7));
    await new Promise((r) => setTimeout(r, 5));
    return midi.usbOut();
  };
  return { midi, ask };
}
const bodyOf = (hex: string) => decode8in7(Uint8Array.from(hex.slice(14, -2).match(/../g)!, (h) => parseInt(h, 16)));

it('answers INFO on the emulator link and nothing on an empty drain', async () => {
  const { midi, ask } = await emulator([]);
  expect(midi.usbOut()).toBe('');
  const hex = await ask(CMD.INFO, 5);
  expect(hex.startsWith('f07d646b01' + (CMD.INFO | 0x40).toString(16).padStart(2, '0') + '05')).toBe(true);
  expect(midi.usbOut()).toBe('');
});

it('with digikit/debug on the card INFO advertises LOG, PEEK and LOGCFG and LOGCFG answers in the sdk.toml format', async () => {
  const { ask } = await emulator(['digikit/debug']);
  const info = bodyOf(await ask(CMD.INFO, 5));
  expect(new DataView(info.buffer, info.byteOffset).getUint32(info.length - 8 - 4, false) & DEBUG_CAPS).toBe(
    DEBUG_CAPS,
  );
  const reply = await ask(SERVICE_CMD.LOGCFG, 6, [0]);
  expect(reply.startsWith('f07d646b01' + (SERVICE_CMD.LOGCFG | 0x40).toString(16) + '06')).toBe(true);
  expect(Array.from(bodyOf(reply))).toEqual([0, 2, ...Array(8).fill(0xff), 1]); // status, default level, 8 module levels, debug_on
});

it('without digikit/debug on the card INFO has no debug capability and the commands are not answered', async () => {
  const { ask } = await emulator(['test/settings-a']);
  const info = bodyOf(await ask(CMD.INFO, 5));
  expect(new DataView(info.buffer, info.byteOffset).getUint32(info.length - 8 - 4, false) & DEBUG_CAPS).toBe(0);
  for (const cmd of [SERVICE_CMD.LOG, SERVICE_CMD.PEEK, SERVICE_CMD.LOGCFG]) expect(await ask(cmd, 6, [0])).toBe('');
});
