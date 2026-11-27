import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, test } from 'vitest';
import type { Device } from '@digi/test-support';
import { cardStore, deviceProfile, fxProfile } from '../runtime/device-data.ts';
import { selectDevices } from '../runtime/devices.ts';
import { addedFactorOf, pcmStats } from '../runtime/pcm-files.ts';
import { addedFactor } from '../runtime/pcm-compare.ts';
import { decodeSettings, lockCheck, seedSettingsCard, SLOT0, STAMP } from '../runtime/project-kit.ts';
import { encodeOverlay } from '../runtime/write-safety.ts';

// The data and analysis helpers of the FX route suites, which were the Python tools efcc91f7e:tools/patchkit/pk_digikit_fx*.py: the device facts, the settings seed of a pristine card, the lock leg's project
// bytes and the send's added factor. Each is checked against the device files or an independent computation.

const SECTOR = 512;
const dirs: string[] = [];
const tmp = () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-fx-data-'));
  dirs.push(dir);
  return dir;
};
afterEach(() => {
  for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

/** The device file as plain data (the independent reading the profile is compared with). */
const toml = (device: Device) => deviceProfile(device) as unknown as Record<string, any>;

describe.each(selectDevices())('%s FX profile', (device) => {
  test('the profile is the device file: limits, [fx], and the card store sectors from its end', () => {
    const t = toml(device);
    const profile = fxProfile(device);
    expect([profile.fx_slots, profile.send_slots]).toEqual([t.limits.fx_slots, t.limits.send_slots]);
    expect(profile.fx).toEqual(t.fx);
    const store = t.card.store;
    // the selection copies are the last two sectors of the store, the settings copies (two of half the area each) lie before them
    const settings = store.settings_sectors ?? 32;
    expect(profile.selection).toEqual({ copy_a: store.end_sector - 2, copy_b: store.end_sector - 1 });
    expect(profile.settings.copy_a).toBe(store.end_sector - 2 - settings);
    expect(profile.settings.copy_b - profile.settings.copy_a).toBe(settings / 2);
    expect(profile.kit_areas).toEqual(t.fx.kit_areas);
    expect(profile.value_classes).toBeGreaterThan(0x40000000);
  });
});

describe('seedSettingsCard', () => {
  /** A sparse card file just large enough for the two settings copies of `device`. */
  const card = (device: Device, sectors?: number) => {
    const file = path.join(tmp(), 'card.img');
    const store = cardStore(device);
    fs.closeSync(fs.openSync(file, 'w'));
    fs.truncateSync(file, (sectors ?? store.setB + store.setCopySectors) * SECTOR);
    return file;
  };
  const sectorsOf = (file: string, first: number, count: number) => {
    const fd = fs.openSync(file, 'r');
    try {
      const out = Buffer.alloc(count * SECTOR);
      fs.readSync(fd, out, 0, out.length, first * SECTOR);
      return out;
    } finally {
      fs.closeSync(fd);
    }
  };

  test.each(selectDevices())(
    '%s: copy A holds the entries as a valid record of seq 1, copy B stays empty',
    (device) => {
      const file = card(device);
      const entries: [string, string][] = [
        ['fx/route/test/effect', '01'],
        ['fx/send/test/effect', '00ff7f'],
      ];
      const store = cardStore(device);
      const result = seedSettingsCard(file, device, entries);
      expect(result).toMatchObject({ card: file, sector: store.setA, seq: 1, entries: 2 });
      expect(result.bytes % SECTOR).toBe(0);
      const record = decodeSettings(sectorsOf(file, store.setA, store.setCopySectors));
      expect(record?.seq).toBe(1);
      expect(record?.entries.map(([key, value]) => [key, value.toString('hex')])).toEqual(entries);
      expect(sectorsOf(file, store.setB, store.setCopySectors).every((b) => b === 0)).toBe(true);
    },
  );

  test('a card with a written copy, a card that ends early and an entry that breaks a limit are refused before a byte is written', () => {
    const device = selectDevices()[0]!;
    const store = cardStore(device);
    const used = card(device);
    seedSettingsCard(used, device, [['a', '01']]);
    expect(() => seedSettingsCard(used, device, [['a', '01']])).toThrow('must be empty');
    const small = card(device, store.setB);
    expect(() => seedSettingsCard(small, device, [['a', '01']])).toThrow('too small');
    const fresh = card(device);
    for (const [what, entry] of [
      ['a key with a space', ['a b', '01']],
      ['a key past the limit', ['k'.repeat(48), '01']],
      ['an odd hex value', ['a', '0']],
      ['a value past the limit', ['a', '00'.repeat(1025)]],
    ] as const)
      expect(() => seedSettingsCard(fresh, device, [[entry[0], entry[1]]]), what).toThrow();
    expect(sectorsOf(fresh, store.setA, store.setCopySectors).every((b) => b === 0)).toBe(true);
  });
});

describe('seedSettingsCard guards', () => {
  const sparse = (device: Device, sectors: number) => {
    const file = path.join(tmp(), 'card.img');
    fs.closeSync(fs.openSync(file, 'w'));
    fs.truncateSync(file, sectors * SECTOR);
    return file;
  };
  const device = selectDevices()[0]!;
  const store = cardStore(device);
  const size = (store.setB + store.setCopySectors) * SECTOR;

  const read = (file: string, first: number, count: number) => {
    const fd = fs.openSync(file, 'r');
    try {
      const out = Buffer.alloc(count * SECTOR);
      fs.readSync(fd, out, 0, out.length, first * SECTOR);
      return out;
    } finally {
      fs.closeSync(fd);
    }
  };
  const copies = (file: string) => read(file, store.setA, store.setB + store.setCopySectors - store.setA);

  test('a seed keeps the bytes outside copy A and the rest of copy A empty', () => {
    const file = sparse(device, store.setB + store.setCopySectors);
    const fd = fs.openSync(file, 'r+');
    fs.writeSync(fd, Buffer.from('caller-owned'), 0, 12, 0);
    fs.closeSync(fd);
    const result = seedSettingsCard(file, device, [['a', '01']]);
    expect(fs.statSync(file).size).toBe(size);
    expect(read(file, 0, 1).toString('latin1', 0, 12)).toBe('caller-owned');
    const own = read(file, store.setA, result.bytes / SECTOR);
    const record = decodeSettings(own)!;
    expect(own.subarray(record.used).every((b) => b === 0)).toBe(true);
    expect(read(file, store.setB, store.setCopySectors).every((b) => b === 0)).toBe(true);
  });

  test('an empty list seeds an empty record; a card that is too small, or a non-empty copy B, is refused untouched', () => {
    const fresh = sparse(device, store.setB + store.setCopySectors);
    expect(seedSettingsCard(fresh, device, [])).toMatchObject({ entries: 0, seq: 1 });
    const short = sparse(device, store.setB);
    expect(() => seedSettingsCard(short, device, [])).toThrow('too small');
    expect(fs.statSync(short).size).toBe(store.setB * SECTOR);
    const occupied = sparse(device, store.setB + store.setCopySectors);
    const fd = fs.openSync(occupied, 'r+');
    fs.writeSync(fd, Buffer.from('x'), 0, 1, (store.setB + 1) * SECTOR - 1);
    fs.closeSync(fd);
    expect(() => seedSettingsCard(occupied, device, [])).toThrow('must be empty');
    expect(read(occupied, store.setA, store.setCopySectors).every((b) => b === 0)).toBe(true);
  });

  test.each([
    [
      'a duplicate key',
      [
        ['a', '00'],
        ['a', '00'],
      ],
    ],
    ['a non-ASCII key', [['\u00e9', '']]],
    ['an empty key', [['', '00']]],
    ['a value past the limit', [['a', '00'.repeat(1025)]]],
    [
      'entries past the settings area',
      Array.from({ length: 20 }, (_, i) => [`${'k'.repeat(40)}${i.toString().padStart(2, '0')}`, '00'.repeat(1024)]),
    ],
  ] as [string, [string, string][]][])('%s is refused before a byte is written', (_what, entries) => {
    const file = sparse(device, store.setB + store.setCopySectors);
    expect(() => seedSettingsCard(file, device, entries)).toThrow();
    expect(copies(file).every((b) => b === 0)).toBe(true);
  });
});

describe('lockCheck', () => {
  /** An overlay that writes `byte` at offset `at` of sector `sector`. */
  const overlay = (writes: [sector: number, at: number, byte: number][]) => {
    const file = path.join(tmp(), 'overlay.bin');
    const sectors = new Map<number, Buffer>();
    for (const [sector, at, byte] of writes) {
      const data = sectors.get(sector) ?? Buffer.alloc(SECTOR);
      data[at] = byte;
      sectors.set(sector, data);
    }
    fs.writeFileSync(file, encodeOverlay(sectors));
    return file;
  };

  test('the trig and the send turn change pattern 0 as written, and the amount turn differs from the send turn by its own bytes', () => {
    const file = path.join(tmp(), 'card.img');
    fs.closeSync(fs.openSync(file, 'w'));
    fs.truncateSync(file, (SLOT0 + 0x1000) * SECTOR);
    // pattern 0 starts at slot byte 0x200: its bytes live in the sectors SLOT0 + 1 on; the header (sector SLOT0) holds the stamp at 0x18
    const pattern = SLOT0 + 1;
    const result = lockCheck(selectDevices()[0]!, file, {
      base: overlay([[SLOT0, 0, 1]]),
      none: overlay([
        [SLOT0, 0, 1],
        [pattern, 4, 9],
        [pattern, 5, 9],
      ]),
      send: overlay([
        [SLOT0, 0, 1],
        [pattern, 4, 9],
        [pattern, 5, 9],
      ]),
      amount: overlay([
        [SLOT0, 0, 1],
        [SLOT0, STAMP[0]![0], 7], // the stamp is ignored
        [pattern, 4, 9],
        [pattern, 5, 9],
        [SLOT0 + 0x20, 100, 3], // a byte in the pattern region of another record
      ]),
    });
    // two bytes of pattern 0 differ between the base and the trig overlay; the send overlay repeats the trig's bytes
    expect(result.trig_changed_bytes).toBe(2);
    expect(result.send_changed_bytes).toBe(0);
    expect(result.amount_diff.differBytes).toEqual({ pattern: 1 });
    expect(result.amount_diff.equal).toBe(false);
  });
});

describe('lockCheck refusals', () => {
  test('a torn overlay is refused and the card is not touched', () => {
    const dir = tmp();
    const overlay = path.join(dir, 'torn.bin');
    fs.writeFileSync(overlay, 'torn');
    const card = path.join(dir, 'card.img');
    fs.writeFileSync(card, 'original');
    const all = { base: overlay, none: overlay, send: overlay, amount: overlay };
    expect(() => lockCheck(selectDevices()[0]!, card, all)).toThrow();
    expect(fs.readFileSync(card, 'latin1')).toBe('original');
    expect(fs.readdirSync(dir).sort()).toEqual(['card.img', 'torn.bin']);
  });

  test('a good check leaves the card bytes as they were', () => {
    const dir = tmp();
    const card = path.join(dir, 'card.img');
    const bytes = Buffer.alloc((SLOT0 + 0x1000) * SECTOR, 7);
    fs.writeFileSync(card, bytes);
    const overlay = path.join(dir, 'o.bin');
    const data = Buffer.alloc(SECTOR);
    data[0] = 8;
    fs.writeFileSync(overlay, encodeOverlay(new Map([[SLOT0 + 1, data]])));
    lockCheck(selectDevices()[0]!, card, { base: overlay, none: overlay, send: overlay, amount: overlay });
    expect(fs.readFileSync(card).equals(bytes)).toBe(true);
    expect(fs.readdirSync(dir).sort()).toEqual(['card.img', 'o.bin']);
  });
});

describe('capture guards (addedFactorOf, pcmStats)', () => {
  const wav = (samples: ArrayLike<number> | null, raw?: Buffer) => {
    const file = path.join(tmp(), 'x.wav');
    if (raw) return (fs.writeFileSync(file, raw), file);
    const body = Buffer.from(Float32Array.from(samples!).buffer);
    const fmt = Buffer.alloc(16);
    fmt.writeUInt16LE(3, 0);
    fmt.writeUInt16LE(2, 2);
    fmt.writeUInt32LE(48000, 4);
    fmt.writeUInt32LE(48000 * 8, 8);
    fmt.writeUInt16LE(8, 12);
    fmt.writeUInt16LE(32, 14);
    const chunk = (tag: string, b: Buffer) => {
      const head = Buffer.alloc(8);
      head.write(tag, 0, 'latin1');
      head.writeUInt32LE(b.length, 4);
      return Buffer.concat([head, b]);
    };
    const chunks = Buffer.concat([Buffer.from('WAVE'), chunk('fmt ', fmt), chunk('data', body)]);
    fs.writeFileSync(file, Buffer.concat([Buffer.from('RIFF'), Buffer.alloc(4), chunks]));
    return file;
  };
  const noise = (n: number) =>
    Float32Array.from({ length: n }, (_, i) => Math.sin(i * 0.37) * Math.cos(i * 0.011) * 0.2);

  test('the factor is recovered from scaled, shifted captures', () => {
    const dry = noise(5000);
    const out = new Float32Array(5000);
    for (let i = 80; i < 5000; i++) out[i] = 1.5 * dry[i - 80]!;
    expect(addedFactorOf(wav(dry), wav(out))).toBeCloseTo(0.5, 5);
  });

  test.each([2, 1200])('a capture of %i samples is too short', (size) => {
    const file = wav(new Float32Array(size).fill(1));
    expect(() => addedFactorOf(file, file)).toThrow('more than 1200 samples');
  });

  test('a silent fit window is refused even when sound lies outside it', () => {
    const samples = new Float32Array(2000);
    samples[0] = 0.25;
    const file = wav(samples);
    expect(() => addedFactorOf(file, file)).toThrow('non-silent reference fit window');
  });

  test.each([
    ['empty', () => wav([]), 'empty PCM'],
    ['NaN', () => wav(Object.assign(new Float32Array(2000).fill(1), { 42: NaN })), 'nonfinite'],
    ['infinite', () => wav(Object.assign(new Float32Array(2000).fill(1), { 42: Infinity })), 'nonfinite'],
    ['malformed', () => wav(null, Buffer.from('not a WAV')), 'not a WAV'],
  ])('%s PCM is refused by both commands', (_what, make, message) => {
    const file = make();
    expect(() => pcmStats(file)).toThrow(message);
    expect(() => addedFactorOf(file, file)).toThrow(message);
  });

  test('pcmStats counts a silent capture', () => {
    expect(pcmStats(wav(new Float32Array(40)))).toEqual({ samples: 40, finite: true, peak: 0 });
    const hit = new Float32Array(40);
    hit[7] = -0.375;
    expect(pcmStats(wav(hit))).toEqual({ samples: 40, finite: true, peak: 0.375 });
  });
});

describe('addedFactor', () => {
  /** A note: a windowed sum of two sines, `n` samples. */
  const note = (n: number) =>
    Float32Array.from({ length: n }, (_, i) => Math.sin(i * 0.05) * 0.3 + Math.sin(i * 0.31) * 0.1);

  test('is the factor f of out = dry * (1 + f), also when the note starts a few samples later', () => {
    const dry = note(6000);
    for (const f of [0.5, 0.0625, -0.25]) {
      const out = dry.map((v) => v * (1 + f));
      expect(addedFactor(dry, out)).toBeCloseTo(f, 5);
      // the output note starts 40 samples later (an even shift): the fit finds the shift
      const late = new Float32Array(dry.length);
      late.set(out.subarray(0, dry.length - 40), 40);
      expect(addedFactor(dry, late)).toBeCloseTo(f, 3);
    }
  });
});
