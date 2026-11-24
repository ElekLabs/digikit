import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, test } from 'vitest';
import type { Device } from '@digi/test-support';
import { cardStore, deviceProfile, fxProfile } from '../runtime/device-data.ts';
import { selectDevices } from '../runtime/devices.ts';
import { addedFactor } from '../runtime/pcm-compare.ts';
import { decodeSettings, lockCheck, seedSettingsCard, SLOT0, STAMP } from '../runtime/project-kit.ts';
import { encodeOverlay } from '../runtime/write-safety.ts';

// The data and analysis helpers of the FX route suites, which were the Python tools pk_digikit_fx*.py: the device facts, the settings seed of a pristine card, the lock leg's project
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
