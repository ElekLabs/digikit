// The Remote page and the Emulator page's faceplate in the preview (`?page=remote`, `?page=emulator`): the device screen is a test
// pattern, so a screenshot shows where and how large the 128 x 64 image is drawn. The Remote page is the app's own code; only the
// DkSession methods that ask the device are replaced.
import { CAP, LOG_KIND } from '@digi/dk-session/gen_proto';
import { createSessionHub } from '@digi/workbench-core/transfer/session-core.ts';
import { deviceOfPort, deviceProfiles } from '@digi/workbench-core/data/device-profiles.ts';
import { cardDevice } from './card-mock';
import { DkSession } from '@digi/dk-session';
import { queryFrame } from './frame-from-png';

/** A recognisable 128 x 64 frame in the firmware's screen layout (`screenPixel`): a frame, corner marks, a diagonal, and a bar of steps. */
export function testPattern(): Uint8Array {
  const buf = new Uint8Array(1024);
  const set = (x: number, y: number) => {
    if (x < 0 || x > 127 || y < 0 || y > 63) return;
    buf[7 - (y >> 3) + 8 * x] |= 1 << (y & 7);
  };
  for (let x = 0; x < 128; x++) {
    set(x, 0);
    set(x, 63);
  }
  for (let y = 0; y < 64; y++) {
    set(0, y);
    set(127, y);
  }
  for (let i = 0; i < 64; i++) set(i * 2, i); // diagonal corner to corner
  for (let i = 0; i < 64; i++) set(127 - i * 2, i);
  for (let k = 0; k < 8; k++)
    for (let y = 56 - k * 2; y < 60; y++) for (let x = 8 + k * 4; x < 11 + k * 4; x++) set(x, y);
  // a letter F in the upper left (5 x 7 at 3x), so a mirror or a rotation is visible
  for (let y = 0; y < 21; y++) set(8, 8 + y);
  for (let x = 0; x < 12; x++) {
    set(8 + x, 8);
    set(8 + x, 18);
  }
  return buf;
}

export function previewSessions() {
  const fallback = testPattern();
  const loaded = queryFrame();
  const unidentified = new URLSearchParams(location.search).has('unidentified');
  const history = new URLSearchParams(location.search).has('history');
  const began = Date.now();
  return createSessionHub((transport, opts, key) => {
    const session = new DkSession(transport, { timeoutMs: 6000, ...opts });
    const profile = deviceProfiles[deviceOfPort(key) ?? cardDevice];
    const version = profile.firmware.at(-1)?.version ?? '';
    return Object.assign(session, {
      rpc: async (type: number) => {
        if (unidentified || new URLSearchParams(location.search).has('runtimeOnly'))
          throw new Error('Unknown MIDI endpoint');
        return type === 1
          ? Uint8Array.of(0, ...new TextEncoder().encode(profile.name), 0)
          : new TextEncoder().encode(version);
      },
      screen: async () => ((await loaded) ?? fallback).slice(),
      info: async () => {
        if (unidentified || new URLSearchParams(location.search).has('stock'))
          throw Object.assign(new Error('No runtime'), { kind: 'timeout' });
        return { port: profile.short + '-' + version, caps: 1 << CAP.LOG, runtime: 11 << 8 };
      },
      logPage: async () => {
        const tick = history ? Math.floor((Date.now() - began) / 5000) : 0;
        const seq = 1 + tick;
        const cpu = [213, 350, 290, 180][tick % 4]!;
        const dsp = [345, 730, 510, 290][tick % 4]!;
        return [
          seq,
          seq,
          [
            {
              seq,
              time: seq * 15000,
              kind: LOG_KIND.HEALTH,
              flags: 0,
              args: [(6 << 16) | cpu, dsp, Math.max(dsp, 420), 0, 0],
            },
          ],
        ];
      },
      remoteState: async () => ({ enabled: true, format: 1, turnLimit: 10 }),
      remoteKey: async () => {},
      remoteTurn: async () => {},
      remoteRelease: async () => {},
    });
  });
}
