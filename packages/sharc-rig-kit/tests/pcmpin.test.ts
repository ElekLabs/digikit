import { mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { wordsHash } from '../src/analysis.ts';
import { fingerprint } from '../src/fingerprint.ts';
import {
  checkCapture,
  compare,
  deviceBlockFrames,
  measure,
  parseFloatWav,
  readPin,
  report,
  sineProblems,
  upgradeV1,
  writePin,
} from '../src/pcmpin.ts';
import { wavBytes } from '../src/plot.ts';

const HZ = 440 * 2 ** (-9 / 12); // MIDI 60
const ONSET = 194;

/** stereo interleaved: silence for ONSET frames, then a sine with `harm3` of a third harmonic; `gain(t)` shapes the level */
function tone(
  o: {
    hz?: number;
    harm3?: number;
    gain?: (t: number) => number;
    frames?: number;
    onset?: number;
    right?: number;
  } = {},
): Float32Array {
  const frames = o.frames ?? 24000;
  const onset = o.onset ?? ONSET;
  const out = new Float32Array(frames * 2);
  for (let i = onset; i < frames; i++) {
    const t = (i - onset) / 48000;
    const f = o.hz ?? HZ;
    const g = (o.gain ?? (() => 1))(t);
    const v = Math.fround(
      g * 0.2 * (Math.sin(2 * Math.PI * f * t) + (o.harm3 ?? 0) * Math.sin(2 * Math.PI * 3 * f * t)),
    );
    out[2 * i] = v;
    out[2 * i + 1] = Math.fround(v * (o.right ?? 1));
  }
  return out;
}

const pinOf = (x: Float32Array) => measure(x, { channels: 2, windowFrames: 12000 }).fp;
const diff = (pin: ReturnType<typeof pinOf>, x: Float32Array) => compare(pin, pinOf(x));

describe('measure', () => {
  it('reads onset, level, stereo match, pitch, crest and harmonics of a sine', () => {
    const fp = pinOf(tone());
    expect(fp.onset).toBe(ONSET + 1); // the first sample of a sine is 0
    expect(fp.frames).toBe(12000);
    expect(fp.stereo).toEqual({ identical: true, max_diff: 0 });
    expect(fp.peak_db).toBeCloseTo(20 * Math.log10(0.2), 1);
    expect(1200 * Math.log2(fp.periodic!.pitch.hz / HZ)).toBeCloseTo(0, 0);
    expect(fp.periodic!.pitch.midi).toBeCloseTo(60, 2);
    expect(fp.periodic!.crest).toBeGreaterThan(1.4);
    expect(fp.periodic!.crest).toBeLessThan(1.43);
    expect(Math.max(...fp.periodic!.harmonics_db)).toBeLessThan(-80);
    expect(sineProblems(fp, { note: 60 })).toEqual([]);
  });

  it('calls noise not periodic', () => {
    let seed = 1;
    const n = Float32Array.from(
      { length: 24000 },
      () => ((seed = (Math.imul(seed, 1103515245) + 12345) >>> 0) / 2 ** 32 - 0.5) * 0.2,
    );
    expect(measure(n, { windowFrames: 12000 }).fp.periodic).toBeNull();
  });

  it('is stable through a JSON round trip', () => {
    const fp = pinOf(tone());
    expect(JSON.parse(JSON.stringify(fp))).toEqual(fp);
  });
});

describe('compare', () => {
  const pin = pinOf(tone());

  it('has no difference for the same signal', () => expect(diff(pin, tone())).toEqual([]));

  it('says a pitch change in cents', () => {
    const d = diff(pin, tone({ hz: HZ * 2 ** (14 / 1200) }));
    expect(d.some((l) => /^pitch \+1[34]\.\d cents/.test(l))).toBe(true);
  });

  it('says a level drop and where it starts', () => {
    const d = diff(pin, tone({ gain: (t) => (t < 0.1 ? 1 : 10 ** (-3.1 / 20)) }));
    expect(d.find((l) => l.startsWith('level'))).toMatch(/^level -3\.\d dB from \d+ ms/);
  });

  it('says a harmonic that rose', () => {
    const d = diff(pin, tone({ harm3: 0.012 }));
    expect(d.some((l) => /^harmonic 3 rose to -3\d/.test(l) || /^harmonic 3 rose to -4\d/.test(l))).toBe(true);
  });

  it('says an onset that moved', () => {
    expect(diff(pin, tone({ onset: ONSET + 32 }))).toContain(
      `onset moved 32 frames (${pin.onset} -> ${pin.onset + 32})`,
    );
  });

  describe('onset-aligned device pins', () => {
    const aligned = (x: Float32Array, block?: number) => compare(pin, pinOf(x), { block });
    it('a whole-block shift passes, either way', () => {
      expect(aligned(tone({ onset: ONSET - 32 }), 32)).toEqual([]);
      expect(aligned(tone({ onset: ONSET + 64 }), 32)).toEqual([]);
    });
    it('a shift that is not whole blocks fails', () => {
      expect(aligned(tone({ onset: ONSET - 20 }), 32)[0]).toContain('onset moved -20 frames');
    });
    it('a 3-block shift fails', () => {
      expect(aligned(tone({ onset: ONSET + 96 }), 32)[0]).toContain('onset moved 96 frames');
    });
    it('a changed sample after the onset fails', () => {
      const x = tone({ onset: ONSET - 32 });
      x[2 * 5000] = Math.fround(x[2 * 5000]! + 0.1);
      expect(aligned(x, 32).length).toBeGreaterThan(0);
    });
    it('without a block size the onset is exact', () => {
      expect(aligned(tone({ onset: ONSET + 32 }))[0]).toContain('onset moved 32 frames');
    });
    it('an exact pin hashes the signal from the onset: a block shift keeps the hash', () => {
      const a = measure(tone(), { channels: 2, windowFrames: 12000 });
      const b = measure(tone({ onset: ONSET - 32 }), { channels: 2, windowFrames: 12000 });
      const exact = { ...a.fp, exact: { sha256: wordsHash(a.window) } };
      expect(compare(exact, b.fp, { sha256: wordsHash(b.window), block: 32 })).toEqual([]);
    });
  });

  it('reads the block of a device from its profile', () => {
    const dir = path.join(import.meta.dirname, '../../../devices');
    const shorts = readdirSync(dir)
      .filter((n) => n.endsWith('.toml'))
      .map((n) => readFileSync(path.join(dir, n), 'utf8'))
      .filter((t) => /^dsp_blocks_per_second/m.test(t))
      .map((t) => /^short = "(\w+)"/m.exec(t)![1]!);
    expect(shorts.length).toBeGreaterThan(1);
    for (const d of shorts) expect(deviceBlockFrames(d)).toBe(32);
    expect(() => deviceBlockFrames('nope')).toThrow();
  });

  it('says that L = R is gone', () => {
    expect(diff(pin, tone({ right: 0.9 })).some((l) => l.startsWith('L and R differ now'))).toBe(true);
  });

  it('accepts what the tolerance allows', () => {
    const loose = { ...pin, tolerance: { level_db: 4, band_db: 40, pitch_cents: 30, crest_pct: 5 } };
    expect(compare(loose, pinOf(tone({ hz: HZ * 2 ** (14 / 1200), gain: () => 0.8 })))).toEqual([]);
  });

  it('an exact pin needs the hash and still lists the fingerprint differences', () => {
    const x = tone();
    const m = measure(x, { channels: 2, windowFrames: 12000 });
    const exact = { ...m.fp, exact: { sha256: wordsHash(m.window) } };
    expect(compare(exact, m.fp, { sha256: wordsHash(m.window) })).toEqual([]);
    const other = compare(exact, m.fp, { sha256: 'beef' });
    expect(other).toHaveLength(1);
    expect(other[0]).toContain('only bits differ');
  });

  it('reads a v1 pin: onset, peak and bands are compared, the rest is skipped', () => {
    const mono = Array.from(tone().filter((_, i) => i % 2 === 0));
    const v1 = fingerprint(mono);
    const now = measure(mono, { blockRms: true }).fp;
    expect(compare(v1, now)).toEqual([]);
    expect(
      compare(
        upgradeV1(v1),
        measure(
          mono.map((v) => v * 0.5),
          { blockRms: true },
        ).fp,
      ).some((l) => l.startsWith('peak -6.0')),
    ).toBe(true);
  });
});

describe('files', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'pcmpin-'));
  afterAll(() => rmSync(dir, { recursive: true, force: true }));

  it('writes a pin, keeps its tolerance on a re-pin and checks a capture against it', () => {
    const file = path.join(dir, 'pins', 'a.pcm.json');
    const x = tone();
    const first = checkCapture(file, x, { name: 'a', channels: 2, windowFrames: 12000, outDir: dir });
    expect([first.ok, first.pending]).toEqual([false, true]);
    writePin(file, first.fp, { tolerance: { pitch_cents: 3 } });
    writePin(file, pinOf(tone({ gain: () => 0.9 })));
    expect(readPin(file)).toMatchObject({ version: 2, window_frames: 12000, tolerance: { pitch_cents: 3 } });
    expect(checkCapture(file, tone({ gain: () => 0.9 }), { name: 'a', channels: 2 }).ok).toBe(true);
    const bad = checkCapture(file, tone({ hz: HZ * 1.01 }), { name: 'a', channels: 2, outDir: dir });
    expect(bad.ok).toBe(false);
    expect(bad.differences.some((l) => l.startsWith('pitch'))).toBe(true);
    expect(report(bad.fp, readPin(file))).toContain('expected');
  });

  it('reads the float WAV of a capture', () => {
    const w = parseFloatWav(wavBytes([0.5, -0.5, 0.25, -0.25], 48000, 2));
    expect([w.rate, w.channels, w.samples]).toEqual([48000, 2, [0.5, -0.5, 0.25, -0.25]]);
  });
});
