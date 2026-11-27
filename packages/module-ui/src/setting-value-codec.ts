// Display value <-> stored bytes of a setting, and the one value rule. Patchkit's `Kind::accepts` is the Rust copy of `accepts`; the
// vectors in runtime/core/vectors/workbench-vectors.json tie the two together.
import type { Setting } from './setting.ts';
import type { AnySettingValue } from './settings-schema.ts';

const encoder = new TextEncoder();
const strict = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });

/** A value that no stored form of the setting can hold. */
export class InvalidSettingValue extends Error {}

const scale = (setting: { decimals: number }) => 10 ** setting.decimals;

function utf8(bytes: Uint8Array): string | undefined {
  try {
    return strict.decode(bytes);
  } catch {
    return undefined;
  }
}

function stored(value: number) {
  const bytes = new Uint8Array(4);
  new DataView(bytes.buffer).setInt32(0, value);
  return bytes;
}

/**
 * Whether `bytes` are a stored value of `setting`: length and UTF-8 for the text kinds (the module's files decide whether a file name exists),
 * range, step from `min` and length for an integer, 0 or 1 for a boolean, an index below the count for a choice.
 */
function accepts(setting: Setting, bytes: Uint8Array): boolean {
  switch (setting.kind) {
    case 'text':
    case 'file-name':
      return bytes.length <= setting.maxBytes && utf8(bytes) !== undefined;
    case 'integer': {
      if (bytes.length !== 4) return false;
      const v = new DataView(bytes.buffer, bytes.byteOffset, 4).getInt32(0);
      return v >= setting.min && v <= setting.max && (v - setting.min) % setting.step === 0;
    }
    case 'boolean':
      return bytes.length === 1 && bytes[0]! <= 1;
    case 'choice':
      return bytes.length === 1 && bytes[0]! < setting.choices.length;
  }
}

/**
 * The stored bytes of a display value. An integer is multiplied by 10^decimals and rounded to the nearest stored value (a tie goes up);
 * binary noise beyond 12 significant digits is dropped first, so 0.07 with two decimals stores 7. Throws `InvalidSettingValue` when the
 * value has the wrong type or the stored form fails `accepts`.
 */
function encode(setting: Setting, value: AnySettingValue): Uint8Array {
  const bytes = toBytes(setting, value);
  if (!accepts(setting, bytes)) throw new InvalidSettingValue(refusal(setting));
  return bytes;
}

/** The display value of stored bytes. Throws `InvalidSettingValue` when `accepts` refuses them. */
function decode(setting: Setting, bytes: Uint8Array): Exclude<AnySettingValue, null> {
  if (!accepts(setting, bytes))
    throw new InvalidSettingValue(`${setting.label} holds a value the device should not store.`);
  switch (setting.kind) {
    case 'text':
    case 'file-name':
      return utf8(bytes)!;
    case 'integer':
      return new DataView(bytes.buffer, bytes.byteOffset, 4).getInt32(0) / scale(setting);
    case 'boolean':
      return bytes[0] === 1;
    case 'choice':
      return setting.choices[bytes[0]!]!;
  }
}

function toBytes(setting: Setting, value: AnySettingValue): Uint8Array {
  const wrong = () => new InvalidSettingValue(`${setting.label} expects a ${expected(setting)}.`);
  switch (setting.kind) {
    case 'text':
    case 'file-name':
      if (typeof value !== 'string') throw wrong();
      return encoder.encode(value);
    case 'integer': {
      if (typeof value !== 'number' || !Number.isFinite(value)) throw wrong();
      const rounded = Math.round(Number((value * scale(setting)).toPrecision(12)));
      if (rounded > 0x7fffffff || rounded < -0x80000000) throw new InvalidSettingValue(refusal(setting));
      return stored(rounded === 0 ? 0 : rounded);
    }
    case 'boolean':
      if (typeof value !== 'boolean') throw wrong();
      return Uint8Array.of(value ? 1 : 0);
    case 'choice': {
      const index = typeof value === 'string' ? setting.choices.indexOf(value) : -1;
      if (index < 0) throw new InvalidSettingValue(`${setting.label} is one of ${setting.choices.join(', ')}.`);
      return Uint8Array.of(index);
    }
  }
}

function expected(setting: Setting) {
  return { text: 'string', 'file-name': 'string', integer: 'number', boolean: 'boolean', choice: 'choice label' }[
    setting.kind
  ];
}

function refusal(setting: Setting): string {
  switch (setting.kind) {
    case 'text':
    case 'file-name':
      return `${setting.label} allows up to ${setting.maxBytes} bytes.`;
    case 'integer': {
      const shown = (v: number) => (v / scale(setting)).toFixed(setting.decimals);
      const unit = setting.unit ? ` ${setting.unit}` : '';
      return `${setting.label} is ${shown(setting.min)} to ${shown(setting.max)}${unit} in steps of ${shown(setting.step)}.`;
    }
    case 'boolean':
      return `${setting.label} is on or off.`;
    case 'choice':
      return `${setting.label} is one of ${setting.choices.join(', ')}.`;
  }
}

export const settingValueCodec = { accepts, encode, decode, refusal };
