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
