// The shared Workbench vectors and the schema each descriptor case declares in TOML, derived with patchkit's field defaults.
import { readFileSync } from 'node:fs';
import { parse } from 'smol-toml';
import type { DescribedSchema, Setting } from '../src/index.ts';

export interface DescriptorCase {
  name: string;
  module: string;
  module_name: string;
  toml: string;
  bytes_hex: string;
}
export interface ValueCase {
  descriptor: string;
  setting: string;
  value_hex: string;
  valid: boolean;
  rule: string;
}

const file = new URL('../../../runtime/core/vectors/workbench-vectors.json', import.meta.url);
export const vectors = JSON.parse(readFileSync(file, 'utf8')) as { descriptors: DescriptorCase[]; values: ValueCase[] };

export const bytesOf = (hex: string) => Uint8Array.from(hex.match(/../g) ?? [], (h) => parseInt(h, 16));

type Declared = Record<string, unknown> & { id: string; label: string; type: string };

function setting(raw: Declared): Setting {
  const base = {
    id: raw.id,
    label: raw.label,
    apply: (raw.apply as 'immediate' | 'next-restart' | undefined) ?? 'immediate',
  };
  switch (raw.type) {
    case 'text':
      return { ...base, kind: 'text', maxBytes: raw.max_bytes as number };
    case 'file-name':
      return {
        ...base,
        kind: 'file-name',
        maxBytes: raw.max_bytes as number,
        extension: raw.extension as string,
        resetLabel: (raw.reset_label as string | undefined) ?? 'None',
      };
    case 'integer':
      return {
        ...base,
        kind: 'integer',
        min: raw.min as number,
        max: raw.max as number,
        step: (raw.step as number | undefined) ?? 1,
        decimals: (raw.decimals as number | undefined) ?? 0,
        unit: (raw.unit as string | undefined) ?? '',
        default: raw.default as number,
      };
    case 'boolean':
      return { ...base, kind: 'boolean', default: raw.default as boolean };
    case 'choice':
      return { ...base, kind: 'choice', choices: raw.choices as string[], default: raw.default as string };
    default:
      throw new Error(`unknown type ${raw.type}`);
  }
}

export function declaredSchema(c: DescriptorCase): DescribedSchema {
  const settings = ((parse(c.toml).setting as Declared[] | undefined) ?? []).map(setting);
  const version = settings.every((s) => s.kind === 'text' || s.kind === 'file-name') ? 1 : 2;
  return { name: c.module_name, version, settings };
}
