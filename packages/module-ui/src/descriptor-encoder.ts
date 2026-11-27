// The writing half of DescriptorDecoder, for a simulated device (the preview): the bytes patchkit's encoder makes for the same schema.
import type { Setting } from './setting.ts';
import type { SettingsSchema } from './settings-schema.ts';

const encoder = new TextEncoder();

const storedLen = (s: Setting) =>
  s.kind === 'text' || s.kind === 'file-name' ? s.maxBytes : s.kind === 'integer' ? 4 : 1;
const tag = { text: 0, 'file-name': 1, integer: 2, boolean: 3, choice: 4 } as const;

/** The descriptor of `schema` under the module name `name`. Version 1 holds only text and file-name settings; any other kind makes it 2. */
export function encodeDescriptor(name: string, schema: Pick<SettingsSchema, 'settings'>): Uint8Array {
  const out: number[] = [];
  const string = (text: string) => {
    const bytes = encoder.encode(text);
    out.push(bytes.length, ...bytes);
  };
  const i32 = (v: number) => out.push((v >>> 24) & 255, (v >>> 16) & 255, (v >>> 8) & 255, v & 255);
  out.push(schema.settings.every((s) => s.kind === 'text' || s.kind === 'file-name') ? 1 : 2);
  string(name);
  out.push(schema.settings.length);
  for (const s of schema.settings) {
    string(s.id);
    string(s.label);
    out.push(storedLen(s), tag[s.kind], s.apply === 'next-restart' ? 1 : 0);
    switch (s.kind) {
      case 'text':
        break;
      case 'file-name':
        string(s.extension);
        string(s.resetLabel);
        break;
      case 'integer':
        i32(s.min);
        i32(s.max);
        i32(s.step);
        out.push(s.decimals);
        i32(s.default);
        string(s.unit);
        break;
      case 'boolean':
        out.push(s.default ? 1 : 0);
        break;
      case 'choice':
        out.push(s.choices.length, s.choices.indexOf(s.default));
        s.choices.forEach(string);
        break;
    }
  }
  return Uint8Array.from(out);
}
