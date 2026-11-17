/**
 * Checks on the DM loads a rig call records (`CallSpec.loads`, the rig crate's read hook). A load is `[pc, address, width]`: the
 * address as a store reports it (L2 at the loader alias 0x28000000 + byte address, DDR as it is) and the width in bytes (1, 2 or 4;
 * a long-word or SIMD load is one entry per word). A rule is data: which loads it covers (`pcs`, else every load), the byte ranges
 * they must lie in and how many bytes they read in all.
 */
import type { CallResult } from './rig.ts';

export type Load = CallResult['stores'][number];

/** `[first, end)` of byte addresses in the form the rig reports */
export type ByteRange = [first: number, end: number];

export interface LoadRule {
  /** names the rule in a failure */
  label: string;
  /** the rule covers the loads of instructions at these pcs (short-word addresses); without it, every load of the call */
  pcs?: readonly number[];
  /** every covered load lies inside one of these ranges (byte addresses, canonical as `Load`) */
  within: readonly ByteRange[];
  /** the covered loads read this many bytes in all */
  bytes?: number;
  /** the covered loads read a multiple of this many bytes in all (0 loads pass) */
  bytesMultipleOf?: number;
  /** every covered load has one of these widths (default 1, 2 and 4) */
  widths?: readonly number[];
}

/** Sorted ranges with the ones that touch or overlap merged: a load that spans two adjacent words lies inside. */
export function mergeRanges(ranges: readonly ByteRange[]): ByteRange[] {
  const sorted = [...ranges].sort((a, b) => a[0] - b[0]);
  const out: ByteRange[] = [];
  for (const [first, end] of sorted) {
    const last = out[out.length - 1];
    if (last && first <= last[1]) last[1] = Math.max(last[1], end);
    else out.push([first, end]);
  }
  return out;
}

function inside(merged: readonly ByteRange[], address: number, width: number): boolean {
  let lo = 0;
  let hi = merged.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const [first, end] = merged[mid]!;
    if (address < first) hi = mid - 1;
    else if (address >= end) lo = mid + 1;
    else return address + width <= end;
  }
  return false;
}

const hex = (n: number): string => n.toString(16);

/** The rules the loads break, one text each (empty when all hold). The loads of a rule are checked in order; a text names the first bad ones. */
export function loadFailures(loads: readonly Load[], rules: readonly LoadRule[]): string[] {
  const failures: string[] = [];
  for (const rule of rules) {
    const pcs = rule.pcs ? new Set(rule.pcs) : null;
    const covered = pcs ? loads.filter(([pc]) => pcs.has(pc)) : loads;
    const merged = mergeRanges(rule.within);
    const widths = rule.widths ?? [1, 2, 4];
    const bad = covered.filter(([, address, width]) => !widths.includes(width) || !inside(merged, address, width));
    if (bad.length)
      failures.push(
        `${rule.label}: ${bad.length} of ${covered.length} loads outside the range: ${bad
          .slice(0, 4)
          .map(([pc, address, width]) => `pc ${hex(pc)} address ${hex(address)} width ${width}`)
          .join(', ')}`,
      );
    const total = covered.reduce((n, [, , width]) => n + width, 0);
    if (rule.bytes !== undefined && total !== rule.bytes)
      failures.push(`${rule.label}: ${total} bytes loaded, expected ${rule.bytes}`);
    if (rule.bytesMultipleOf !== undefined && total % rule.bytesMultipleOf !== 0)
      failures.push(`${rule.label}: ${total} bytes loaded, not a multiple of ${rule.bytesMultipleOf}`);
  }
  return failures;
}

/** A float read from a loaded word (the held period of the replay): the IEEE single the four bytes make. */
export function wordToFloat(word: number): number {
  const view = new DataView(new ArrayBuffer(4));
  view.setUint32(0, word >>> 0, true);
  return view.getFloat32(0, true);
}
