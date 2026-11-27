// The LIFECYCLE event of the runtime's log (docs/INSTRUMENTATION.md "Lifecycle events"): a step of the package lifecycle, begun, ended or noted. The words are
// [op, name_hash, info, status, detail]; `info` holds phase, step, kind and package record, low byte first (sdk/sdk.toml, the kind's comment).
import type { FieldValue } from './runtime-log.ts';

const PHASES = ['begin', 'end', 'note'] as const;
/** The step numbers of `dkrt_lifecycle::Step`. */
export const LIFECYCLE_STEPS: Readonly<Record<number, string>> = {
  1: 'place',
  2: 'link',
  3: 'start',
  4: 'install',
  5: 'activate',
  6: 'attach',
  7: 'detach',
  8: 'deactivate',
  9: 'restore',
  10: 'export_state',
  11: 'import_state',
  12: 'stop',
  13: 'free',
  14: 'dsp_load',
  15: 'can_stop',
  16: 'unbind',
  17: 'park_wait',
  18: 'busy_retry',
  32: 'check',
  33: 'quiesce',
  34: 'unload',
  35: 'load',
  36: 'dsp',
  37: 'done',
};

/** FNV-1a 32 of a package's file name without directory and extension: the `name_hash` a LIFECYCLE event names its package by. */
export function nameHash(file: string): number {
  const leaf = file.slice(file.lastIndexOf('/') + 1);
  const dot = leaf.lastIndexOf('.');
  const stem = dot > 0 ? leaf.slice(0, dot) : leaf;
  let h = 0x811c9dc5;
  for (const b of new TextEncoder().encode(stem)) h = Math.imul(h ^ b, 0x01000193) >>> 0;
  return h;
}

/** The fields of a LIFECYCLE event: `hash` is the package's `nameHash` (the runtime sends no name; the load of the same package names it later in MOD_LOAD). */
export function lifecycleFields(a: readonly number[]): Record<string, FieldValue> {
  const info = a[2] ?? 0;
  const step = (info >>> 8) & 0xff;
  const record = info >>> 24;
  return {
    op: a[0] ?? 0,
    hash: ((a[1] ?? 0) >>> 0).toString(16).padStart(8, '0'),
    phase: PHASES[info & 0xff] ?? String(info & 0xff),
    step: LIFECYCLE_STEPS[step] ?? String(step),
    kind: ((info >>> 16) & 0xff) === 0 ? 'module' : 'service',
    package: record === 0xff ? null : record,
    status: (a[3] ?? 0) | 0,
    detail: a[4] ?? 0,
  };
}
