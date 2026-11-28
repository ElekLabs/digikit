// The preview's Emulator target answers SysEx through `midi.usb.in` / `midi.usb.out` with the responder of the MIDI targets.
import { expect, it, vi } from 'vitest';
import { CMD } from '@digi/dk-session/gen_proto';

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

  // a command the runtime does not have (LOGCFG without the debug module) is answered E_PROTO, not left to time out
  midi.usbIn('dt2', Uint8Array.of(0xf0, 0x7d, 0x64, 0x6b, 1, 0x22, 6, 0xf7));
  expect(midi.usbOut().startsWith('f07d646b01' + (0x22 | 0x40).toString(16) + '06')).toBe(true);
});
