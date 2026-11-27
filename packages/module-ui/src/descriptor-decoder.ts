// The binary descriptor a module registers in init (layout: packages/patchkit/src/build/workbench/descriptor.rs): version, module name,
// count, then per setting `[len+id][len+label][stored_len][kind][apply]` and the body of its kind. A device reports it; a generated schema
// holds the same data, so a page and the generic form read both the same way.
import type { Setting } from './setting.ts';
import type { DescribedSchema } from './settings-schema.ts';
import { settingValueCodec } from './setting-value-codec.ts';

const MAX_BYTES = 512;
const MAX_TEXT_BYTES = 64;
const MAX_FILE_NAME_BYTES = 47;
const MAX_DECIMALS = 4;
const MAX_UNIT_BYTES = 8;
const MAX_CHOICES = 32;
const strict = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });

export class InvalidDescriptor extends Error {}

class Reader {
  at = 0;
  readonly bytes: Uint8Array;
  constructor(bytes: Uint8Array) {
    this.bytes = bytes;
  }
  byte() {
    if (this.at >= this.bytes.length) throw new InvalidDescriptor('Incomplete module descriptor.');
    return this.bytes[this.at++]!;
  }
  i32() {
    if (this.at + 4 > this.bytes.length) throw new InvalidDescriptor('Incomplete module descriptor.');
    const v = new DataView(this.bytes.buffer, this.bytes.byteOffset + this.at, 4).getInt32(0);
    this.at += 4;
    return v;
  }
  u32() {
    return this.i32() >>> 0;
  }
  /** A length-prefixed string; `empty` allows length 0. */
  string(empty = false) {
    const len = this.byte();
    if ((!len && !empty) || this.at + len > this.bytes.length)
      throw new InvalidDescriptor('Invalid module descriptor string.');
    let text: string;
    try {
      text = strict.decode(this.bytes.subarray(this.at, this.at + len));
    } catch {
      throw new InvalidDescriptor('Invalid module descriptor string.');
    }
    this.at += len;
    return text;
  }
}

function refuse(condition: boolean, what: string): void {
  if (condition) throw new InvalidDescriptor(`Invalid ${what}.`);
}

function body(
  r: Reader,
  base: { id: string; label: string; apply: 'immediate' | 'next-restart' },
  kind: number,
  stored: number,
): Setting {
  switch (kind) {
    case 0:
      refuse(stored < 1 || stored > MAX_TEXT_BYTES, 'text setting');
      return { ...base, kind: 'text', maxBytes: stored };
    case 1: {
      const extension = r.string();
      const resetLabel = r.string();
      refuse(stored < 1 || stored > MAX_FILE_NAME_BYTES || !/^[a-z0-9]{1,16}$/.test(extension), 'file setting');
      refuse(stored < extension.length + 2, 'file setting');
      return { ...base, kind: 'file-name', maxBytes: stored, extension, resetLabel };
    }
    case 2: {
      const min = r.i32();
      const max = r.i32();
      const step = r.u32();
      const decimals = r.byte();
      const def = r.i32();
      const unit = r.string(true);
      refuse(stored !== 4 || max <= min || step < 1 || (max - min) % step !== 0, 'integer setting');
      refuse(
        decimals > MAX_DECIMALS || unit.length > MAX_UNIT_BYTES || !/^[\x20-\x7e]*$/.test(unit),
        'integer setting',
      );
      const setting: Setting = { ...base, kind: 'integer', min, max, step, decimals, unit, default: def };
      refuse(
        !settingValueCodec.accepts(setting, Uint8Array.of(def >>> 24, def >>> 16, def >>> 8, def)),
        'integer default',
      );
      return setting;
    }
    case 3: {
      const def = r.byte();
      refuse(stored !== 1 || def > 1, 'boolean setting');
      return { ...base, kind: 'boolean', default: def === 1 };
    }
    case 4: {
      const count = r.byte();
      const def = r.byte();
      refuse(stored !== 1 || count < 1 || count > MAX_CHOICES || def >= count, 'choice setting');
      const choices = Array.from({ length: count }, () => r.string());
      refuse(new Set(choices).size !== count, 'choice setting');
      return { ...base, kind: 'choice', choices, default: choices[def]! };
    }
    default:
      throw new InvalidDescriptor('Unsupported module setting.');
  }
}

/** Reads a descriptor. Throws `InvalidDescriptor` for truncated or surplus bytes, an unknown version or kind, a kind that its version does not hold, and a setting that breaks patchkit's bounds. */
export function decodeDescriptor(bytes: Uint8Array): DescribedSchema {
  if (bytes.length > MAX_BYTES) throw new InvalidDescriptor('Unsupported module descriptor.');
  const r = new Reader(bytes);
  const version = r.byte();
  if (version !== 1 && version !== 2) throw new InvalidDescriptor('Unsupported module descriptor.');
  const name = r.string();
  const count = r.byte();
  const settings: Setting[] = [];
  for (let i = 0; i < count; i++) {
    const id = r.string();
    const label = r.string();
    const stored = r.byte();
    const kind = r.byte();
    const timing = r.byte();
    refuse(timing > 1 || settings.some((s) => s.id === id), 'public setting');
    if (version === 1 && kind > 1)
      throw new InvalidDescriptor('A version 1 descriptor holds only text and file-name settings.');
    settings.push(body(r, { id, label, apply: timing ? 'next-restart' : 'immediate' }, kind, stored));
  }
  if (r.at !== bytes.length) throw new InvalidDescriptor('Unexpected bytes in module descriptor.');
  return { name, version, settings };
}
