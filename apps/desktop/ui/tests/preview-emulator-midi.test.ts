// The preview's Emulator target answers SysEx through `midi.usb.in` / `midi.usb.out` with the responder of the MIDI targets.
import { expect, it, vi } from 'vitest';
import { decode8in7, encode8in7 } from '@digi/dk-session';
import { CAP, CMD, SERVICE_CMD } from '@digi/dk-session/gen_proto';

const fixture = { dev: 'dt2', state: 'runtime', os: '1.17', flashMs: 0, goneMs: 0, sent: 0 };

it('answers INFO on the emulator link and nothing on an empty drain', async () => {
  vi.stubGlobal('location', { search: '' }); // the preview's sim reads the page query
  const { createFirmwareMidi } = await import('../preview/firmware-midi');
  const midi = createFirmwareMidi(new URLSearchParams(), fixture);
  expect(midi.usbOut()).toBe('');
  midi.usbIn('dt2', Uint8Array.of(0xf0, 0x7d, 0x64, 0x6b, 1, CMD.INFO, 5, 0xf7));
  const hex = midi.usbOut();
  expect(hex.startsWith('f07d646b01' + (CMD.INFO | 0x40).toString(16).padStart(2, '0') + '05')).toBe(true);
  expect(midi.usbOut()).toBe('');

  // the simulated card has digikit/debug: INFO advertises LOG, PEEK and LOGCFG, and LOGCFG answers in the sdk.toml format
  midi.usbIn('dt2', Uint8Array.of(0xf0, 0x7d, 0x64, 0x6b, 1, SERVICE_CMD.LOGCFG, 6, ...encode8in7(Uint8Array.of(0)), 0xf7));
  const reply = midi.usbOut();
  expect(reply.startsWith('f07d646b01' + (SERVICE_CMD.LOGCFG | 0x40).toString(16) + '06')).toBe(true);
  const bytes = decode8in7(Uint8Array.from(reply.slice(14, -2).match(/../g)!, (h) => parseInt(h, 16)));
  expect(Array.from(bytes)).toEqual([0, 2, ...Array(8).fill(0xff), 1]); // status, default level, 8 module levels, debug_on
});

it('without digikit/debug INFO has no LOG, PEEK or LOGCFG capability and none is answered', async () => {
  vi.stubGlobal('location', { search: '' });
  const { createFirmwareMidi } = await import('../preview/firmware-midi');
  const midi = createFirmwareMidi(new URLSearchParams('log=nodebug'), fixture);
  midi.usbIn('dt2', Uint8Array.of(0xf0, 0x7d, 0x64, 0x6b, 1, CMD.INFO, 5, 0xf7));
  const info = decode8in7(Uint8Array.from(midi.usbOut().slice(14, -2).match(/../g)!, (h) => parseInt(h, 16)));
  const caps = new DataView(info.buffer, info.byteOffset).getUint32(info.length - 8 - 4, false);
  expect(caps & ((1 << CAP.LOG) | (1 << CAP.PEEK) | (1 << CAP.LOGCFG))).toBe(0);
  midi.usbIn('dt2', Uint8Array.of(0xf0, 0x7d, 0x64, 0x6b, 1, SERVICE_CMD.LOGCFG, 6, ...encode8in7(Uint8Array.of(0)), 0xf7));
  expect(midi.usbOut()).toBe('');
});
