import { describe, expect, it } from 'vitest';
import { InvalidSettingValue, settingValueCodec as codec, type Setting } from '../src/index.ts';
import { bytesOf, declaredSchema, vectors } from './vectors.ts';

describe('settingValueCodec.accepts against the shared vectors', () => {
  for (const c of vectors.values) {
    it(`${c.descriptor} ${c.setting} ${c.value_hex || '(empty)'}: ${c.rule}`, () => {
      const schema = declaredSchema(vectors.descriptors.find((d) => d.name === c.descriptor)!);
      const setting = schema.settings.find((s) => s.id === c.setting)!;
      expect(codec.accepts(setting, bytesOf(c.value_hex))).toBe(c.valid);
    });
  }
});

const gain = (over: Partial<Extract<Setting, { kind: 'integer' }>> = {}): Setting => ({
  id: 'g',
  label: 'Gain',
  kind: 'integer',
  apply: 'immediate',
  min: -100,
  max: 100,
  step: 1,
  decimals: 2,
  unit: 'dB',
  default: 0,
  ...over,
});

describe('integer values', () => {
  it('scales by decimals both ways', () => {
    expect(Array.from(codec.encode(gain(), 0.75))).toEqual([0, 0, 0, 75]);
    expect(codec.decode(gain(), codec.encode(gain(), -0.5))).toBe(-0.5);
  });
  it('rounds to the nearest stored value, a tie going up, and drops binary noise', () => {
    expect(Array.from(codec.encode(gain(), 0.07))).toEqual([0, 0, 0, 7]); // 0.07 * 100 = 7.000000000000001
    expect(Array.from(codec.encode(gain(), 0.004))).toEqual([0, 0, 0, 0]);
    expect(Array.from(codec.encode(gain(), 0.006))).toEqual([0, 0, 0, 1]);
    expect(Array.from(codec.encode(gain(), 0.005))).toEqual([0, 0, 0, 1]);
    expect(Array.from(codec.encode(gain(), 1.005 - 1))).toEqual([0, 0, 0, 1]); // 1.005 - 1 = 0.00499999999999989
    expect(Array.from(codec.encode(gain(), -0.005))).toEqual([0, 0, 0, 0]);
    expect(codec.encode(gain(), -0)).toEqual(new Uint8Array(4));
  });
  it('refuses a value outside the range or off the step, and a non-number', () => {
    expect(() => codec.encode(gain(), 1.01)).toThrow(/-1\.00 to 1\.00 dB/);
    expect(() => codec.encode(gain({ step: 5 }), 0.07)).toThrow(InvalidSettingValue);
    expect(codec.encode(gain({ step: 5 }), 0.05)).toEqual(Uint8Array.of(0, 0, 0, 5));
    expect(() => codec.encode(gain(), NaN)).toThrow(InvalidSettingValue);
    expect(() => codec.encode(gain(), Infinity)).toThrow(InvalidSettingValue);
    expect(() => codec.encode(gain(), '1')).toThrow(/expects a number/);
  });
  it('holds the i32 extremes and refuses one past them', () => {
    const wide = gain({ min: -2147483648, max: 2147483647, decimals: 0 });
    expect(Array.from(codec.encode(wide, -2147483648))).toEqual([0x80, 0, 0, 0]);
    expect(Array.from(codec.encode(wide, 2147483647))).toEqual([0x7f, 0xff, 0xff, 0xff]);
    expect(codec.decode(wide, Uint8Array.of(0x80, 0, 0, 0))).toBe(-2147483648);
    expect(() => codec.encode(wide, 2147483648)).toThrow(InvalidSettingValue);
    expect(() => codec.encode(wide, -2147483649)).toThrow(InvalidSettingValue);
  });
  it('decodes four decimals', () => {
    expect(codec.decode(gain({ decimals: 4, min: 0, max: 20000 }), Uint8Array.of(0, 0, 0, 1))).toBe(0.0001);
  });
});

describe('the other kinds', () => {
  const base = { id: 's', label: 'S', apply: 'immediate' } as const;
  const text: Setting = { ...base, kind: 'text', maxBytes: 4 };
  const flag: Setting = { ...base, kind: 'boolean', default: false };
  const names = Array.from({ length: 32 }, (_, i) => `c${i}`);
  const mode: Setting = { ...base, kind: 'choice', choices: names, default: 'c0' };

  it('text counts UTF-8 bytes and keeps empty', () => {
    expect(codec.decode(text, codec.encode(text, '🍋'))).toBe('🍋');
    expect(codec.encode(text, '')).toHaveLength(0);
    expect(() => codec.encode(text, '🍋a')).toThrow(/up to 4 bytes/);
    expect(() => codec.encode(text, null)).toThrow(/expects a string/);
    expect(() => codec.decode(text, Uint8Array.of(0xff))).toThrow(InvalidSettingValue);
  });
  it('boolean is one byte', () => {
    expect(codec.encode(flag, true)).toEqual(Uint8Array.of(1));
    expect(codec.decode(flag, Uint8Array.of(0))).toBe(false);
    expect(() => codec.encode(flag, 1)).toThrow(/expects a boolean/);
    expect(() => codec.decode(flag, Uint8Array.of(2))).toThrow(InvalidSettingValue);
  });
  it('choice maps label and index for all 32 labels', () => {
    expect(codec.encode(mode, 'c31')).toEqual(Uint8Array.of(31));
    expect(codec.decode(mode, Uint8Array.of(31))).toBe('c31');
    expect(() => codec.encode(mode, 'c32')).toThrow(/one of/);
    expect(() => codec.decode(mode, Uint8Array.of(32))).toThrow(InvalidSettingValue);
  });
});
