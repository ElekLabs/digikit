// The shared Workbench vectors. `expected_schema` is the `schema` constant patchkit generates for the case's TOML.
import { readFileSync } from 'node:fs';
import type { DescribedSchema, SettingsSchema } from '../src/index.ts';

export interface DescriptorCase {
  name: string;
  module: string;
  module_name: string;
  toml: string;
  bytes_hex: string;
  expected_schema: SettingsSchema;
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

/** What a device's descriptor decodes to: the generated schema without the module id (a descriptor has none), under the module's name. */
export function describedSchema(c: DescriptorCase): DescribedSchema {
  const { module: _module, ...rest } = c.expected_schema;
  return { ...rest, name: c.module_name };
}
