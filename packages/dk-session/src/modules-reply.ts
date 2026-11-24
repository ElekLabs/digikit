// The MODULES reply (runtime 0.11.5): its fixed part per record ends at dsp_len, then `u8 n_sections` and per section `u32 service name hash, u8 n, n words`. The runtime reads no
// meaning into the words; this file does, for the two services a host knows: `machines` (five words) and `fx` (one word per effect row). The fixture
// tests/fixtures/protocol/modules-reply.json is written by the runtime's encoder test and read by every decoder (here and tools/device/dkupload.py).
import { CAP } from './gen_proto.ts';
import type { DkEffectRow, DkModule } from './dkupload-core.ts';

/** What a host says when the device's MODULES records end at `dsp_len` (a runtime before 0.11.5 does not set the `MODULE_SECTIONS` capability). */
export const OLD_MODULES = 'Update the firmware: this runtime lists modules in an older layout';

/** FNV-1a 32 of a service name: the hash a section is named by (`dkneeds::keys::name_hash`). */
export function serviceHash(name: string): number {
  let h = 0x811c9dc5;
  for (const c of new TextEncoder().encode(name)) h = Math.imul(h ^ c, 0x01000193) >>> 0;
  return h;
}

const MACHINES = serviceHash('machines');
const FX = serviceHash('fx');

/** The bytes of a record before its sections (index to dsp_len). */
export const MODULE_FIXED = 61;

/** The machines section: the voice rows, the guard's faults, `nt` with the guard flag in bit 8, then the types 0 to 3 and 4 to 7 packed from the low byte. */
function machines(words: number[]): Pick<DkModule, 'rows' | 'types' | 'faults' | 'guard_off'> {
  const [rows = 0, faults = 0, nt = 0, lo = 0, hi = 0] = words;
  const types: number[] = [];
  for (let i = 0; i < Math.min(nt & 0xff, 8); i++) types.push(((i < 4 ? lo : hi) >>> (8 * (i % 4))) & 0xff);
  return { rows, types, faults, guard_off: (nt & 0x100) !== 0 };
}

/** The fx section: a word for each effect row of the module: row (bits 0 to 7), then four bits each of the route it runs on, the saved route, the allowed routes and the default. */
function effects(words: number[]): DkEffectRow[] {
  return words.map((w) => ({
    row: w & 0xff,
    running: (w >>> 8) & 0xf,
    saved: (w >>> 12) & 0xf,
    allowed: (w >>> 16) & 0xf,
    dflt: (w >>> 20) & 0xf,
  }));
}

/**
 * One page of the MODULES reply (the body after the status): the number of loaded modules and the records of this page. `caps` is the INFO capability word of the
 * device: without the `MODULE_SECTIONS` bit the records are of the older layout and are refused with `OLD_MODULES`, never read as the new one.
 */
export function decodeModulesPage(b: Uint8Array, caps: number | null): { total: number; modules: DkModule[] } {
  if (caps === null || ((caps >>> CAP.MODULE_SECTIONS) & 1) === 0) throw new Error(OLD_MODULES);
  const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
  const total = b[16]!;
  const n = b[17]!;
  const modules: DkModule[] = [];
  let at = 18;
  for (let i = 0; i < n; i++) {
    const name = new TextDecoder().decode(b.subarray(at + 1, at + 33)).split('\0')[0]!;
    const token = v.getUint32(at + 41);
    const sections: Record<string, number[]> = {};
    let p = at + MODULE_FIXED;
    const ns = b[p++]!;
    for (let s = 0; s < ns; s++) {
      const hash = v.getUint32(p);
      const count = b[p + 4]!;
      p += 5;
      const words: number[] = [];
      for (let k = 0; k < count; k++, p += 4) words.push(v.getUint32(p));
      sections[hash.toString(16).padStart(8, '0')] = words;
    }
    modules.push({
      index: b[at]!,
      name,
      version: v.getUint32(at + 33),
      needs: v.getUint32(at + 37),
      ...(token ? { uiInstance: token } : {}),
      cf_base: v.getUint32(at + 45),
      cf_size: v.getUint32(at + 49),
      dsp_off: v.getUint32(at + 53),
      dsp_len: v.getUint32(at + 57),
      sections,
      ...machines(sections[MACHINES.toString(16).padStart(8, '0')] ?? []),
      effects: effects(sections[FX.toString(16).padStart(8, '0')] ?? []),
    });
    at = p;
  }
  return { total, modules };
}
