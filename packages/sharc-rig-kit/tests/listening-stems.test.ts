import { expect, test } from 'vitest';
import { LISTENING_SCENES, listeningStems } from '../src/listening-stems.ts';

test.each(LISTENING_SCENES)('%s is deterministic and keeps the backing on its own stem', (scene) => {
  const a = listeningStems(scene);
  const b = listeningStems(scene);
  expect(a.dry.length % 32).toBe(0);
  expect(Buffer.from(a.dry.buffer).equals(Buffer.from(b.dry.buffer))).toBe(true);
  expect(a.backing.some((sample) => sample !== 0)).toBe(true);
  expect(a.send.some((sample) => sample !== 0)).toBe(true);
  expect(a.send.subarray(0, 4000).every((sample) => sample === 0)).toBe(true);
  expect(a.dry.every((sample, i) => sample === Math.fround(a.backing[i]! + a.send[i]!))).toBe(true);
});

test('the sustained scene has a different send stem from the percussion scene', () => {
  const a = listeningStems('send/percussion');
  const b = listeningStems('send/texture');
  const window = 48000;
  expect(Buffer.from(a.backing.slice(0, window).buffer).equals(Buffer.from(b.backing.slice(0, window).buffer))).toBe(
    true,
  );
  expect(Buffer.from(a.send.slice(0, window).buffer).equals(Buffer.from(b.send.slice(0, window).buffer))).toBe(false);
});

test('repeated listening stems preserve the exact source samples in each cycle', () => {
  const single = listeningStems('send/texture');
  const repeated = listeningStems('send/texture', 3);
  expect(repeated.beats).toBe(single.beats * 3);
  for (const key of ['backing', 'send', 'dry'] as const) {
    const length = single[key].length;
    const reference = Buffer.from(single[key].buffer);
    expect(repeated[key].length).toBe(3 * length);
    for (let cycle = 0; cycle < 3; cycle++)
      expect(Buffer.from(repeated[key].slice(cycle * length, (cycle + 1) * length).buffer).equals(reference)).toBe(
        true,
      );
  }
});

test.each([0, 1.5, 9])('invalid repeat %s cannot generate a scene', (repeat) => {
  expect(() => listeningStems('send/texture', repeat)).toThrow('Scene repeat');
});
