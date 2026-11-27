// The store path of a module and the group of a built .dkm: the one rule of Transfer, kept apart from the transfer client so that the test helpers that
// need only the path do not pull the client (and its edits) into every device test.
import { DkError, HEADER_BYTES, H_ROLE, RECORD_KIND, TABLE_MAGIC, TABLE_VERSION } from '@digi/dk-session';

export type Group = 'Machines' | 'Effects' | 'Filters' | 'LFO shapes' | 'Tools' | 'Services';

const KEYS_TAG = 3; // the KEYS entry of the header table (packages/dkneeds/src/keys.rs)

/** Readers accept the header table versions 1 to TABLE_VERSION (a service is written with 2, a module with 1). */
export const tableVersionOk = (version: number): boolean => version >= 1 && version <= TABLE_VERSION;

/** The group of a .dkm: Services when its role word (header 0x38) is 1, Machines when the KEYS entry of its header table (the header's 0x90 pair) has machine keys, Effects when it has effect keys, LFO shapes when it has LFO shape keys, else Tools.
 * Without the table (a header alone, or the table of runtime 0.7) it is Tools. */
export function groupOf(f: Uint8Array): Group {
  const v = new DataView(f.buffer, f.byteOffset, f.length);
  if (f.length < HEADER_BYTES) return 'Tools';
  const off = v.getUint32(0x90);
  const size = v.getUint32(0x94);
  if (size < 8 || off + size > f.length || v.getUint32(off) !== TABLE_MAGIC || !tableVersionOk(v.getUint16(off + 4)))
    return 'Tools';
  if (v.getUint32(H_ROLE) === 1) return 'Services';
  const kinds = new Set<number>();
  let at = off + 8;
  for (let i = v.getUint16(off + 6); i > 0 && at + 8 <= off + size; i--) {
    const tag = v.getUint16(at);
    const count = v.getUint16(at + 2);
    const len = v.getUint32(at + 4);
    if (tag === KEYS_TAG) {
      let g = at + 8;
      for (let j = 0; j < count && g + 8 <= off + size; j++) {
        const n = v.getUint32(g + 4);
        if (n) kinds.add(v.getUint32(g));
        g += 8 + 4 * n;
      }
    }
    at += 8 + len;
  }
  if (kinds.has(RECORD_KIND.MACHINE)) return 'Machines';
  if (kinds.has(RECORD_KIND.EFFECT)) return 'Effects';
  if (kinds.has(RECORD_KIND.LFO_SHAPE)) return 'LFO shapes';
  return 'Tools';
}

/** The folder of each kind of module under `modules/` (docs/design/taxonomy.md, "Card layout"). */
export const KIND_FOLDER: Record<Exclude<Group, 'Services'>, string> = {
  Machines: 'machine',
  Effects: 'fx',
  Filters: 'filter',
  'LFO shapes': 'lfo-shape',
  Tools: 'tool',
};

/** The longest module id: the module name the runtime keeps in 32 bytes, NUL included (`NAME_MAX` of the module header). */
export const MODULE_ID_LIMIT = 31;
/** The longest path of a selection entry: `SEL_PATH` is 56 bytes with the NUL (`gen_abi.rs`). */
export const SELECTION_PATH_LIMIT = 55;

/** The store path of a module: `modules/<kind>/<author>-<name>.dkm`, the file name the module id with "/" as "-". The kind is the group Transfer shows the module in (its records). A test may give its own `fileName`.
 * Throws a `request` error when the id is longer than 31 bytes or the path longer than 55 characters (a selection entry holds no more). */
export function modulePath(id: string, group: Group = 'Tools', fileName: string = id.replace(/\//g, '-')): string {
  const bytes = new TextEncoder().encode(id).length;
  if (bytes > MODULE_ID_LIMIT)
    throw new DkError({
      error: `the module id "${id}" is ${bytes} bytes; an id may have at most ${MODULE_ID_LIMIT}`,
      kind: 'request',
    });
  const path = `modules/${KIND_FOLDER[group === 'Services' ? 'Tools' : group]}/${fileName}.dkm`;
  if (path.length > SELECTION_PATH_LIMIT)
    throw new DkError({
      error: `the store path ${path} is ${path.length} characters; a selection entry holds at most ${SELECTION_PATH_LIMIT}: shorten the module id`,
      kind: 'request',
    });
  return path;
}
