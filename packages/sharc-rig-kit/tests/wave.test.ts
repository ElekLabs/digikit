import { expect, test } from 'vitest';
import { wavBytes } from '../src/plot.ts';
import { parseWave } from '../src/wave.ts';

test('decodes a standard integer PCM WAV to signed full-scale values', () => {
  // PCM stereo, two frames with 16-bit samples, including one negative full-scale value.
  const wav = Buffer.from(
    '524946462c00000057415645666d7420100000000100020044ac000010b102000400100064617461080000000080004000c00000',
    'hex',
  );
  expect(parseWave(wav)).toEqual({ format: 'pcm', rate: 44100, channels: 2, samples: [-1, 0.5, -0.5, 0] });
});

test('rejects non-finite and truncated rendered WAVs', () => {
  expect(() => parseWave(wavBytes([0, Infinity]))).toThrow('non-finite');
  expect(() => parseWave(wavBytes([0, 1]).subarray(0, -1))).toThrow('Truncated WAV');
});
