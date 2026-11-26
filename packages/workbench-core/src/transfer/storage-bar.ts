// The storage bar above the two Transfer lists: the card's store split like the Files pane (this kind, the others, free), and what the ticked
// items would add. Pure; the drawing is the shared `UsageBar` (ui/components/UsageBar.tsx).
import { formatBytes } from '../modules/backend.ts';
import type { CategoryKey, StoreUsage } from '../target-ops/store.ts';
import type { Mode } from './transfer-core.ts';

/** The store categories that belong to a mode: Modules shows modules and their services, Samples the samples. */
const KINDS: Record<Mode, readonly CategoryKey[]> = { modules: ['modules', 'services'], samples: ['samples'] };
const KIND_LABEL: Record<Mode, string> = { modules: 'Modules', samples: 'Samples' };

export interface BarSegment {
  /** The colour class: a store category, `others`, `preview` or `free`. */
  key: string;
  label: string;
  bytes: number;
}

export interface StorageBar {
  segments: BarSegment[];
  /** The size of the bar: the store's total, else the sum of its parts. */
  base: number;
  free: number;
  /** What the ticked items add, and how much of it the free space cannot hold. */
  added: number;
  overflow: number;
  text: string;
}

/** The bar data for `mode` from the store usage; `added` is the size of the ticked items (0: nothing ticked). */
export function storageBar(usage: StoreUsage, mode: Mode, added = 0): StorageBar {
  const kind = KINDS[mode];
  const segments: BarSegment[] = usage.used
    .filter((u) => kind.includes(u.key))
    .map((u) => ({ key: u.key, label: u.label, bytes: u.bytes }));
  const others =
    usage.used.filter((u) => !kind.includes(u.key)).reduce((a, u) => a + u.bytes, 0) + (usage.overhead ?? 0);
  if (others > 0) segments.push({ key: 'others', label: 'Others', bytes: others });
  const preview = Math.min(added, usage.free);
  if (preview > 0) segments.push({ key: 'preview', label: 'Adds', bytes: preview });
  segments.push({ key: 'free', label: 'Free', bytes: usage.free - preview });
  const base = usage.total ?? segments.reduce((a, s) => a + s.bytes, 0);
  const overflow = Math.max(0, added - usage.free);
  const mine = segments.filter((s) => kind.includes(s.key as CategoryKey)).reduce((a, s) => a + s.bytes, 0);
  const lead = `${KIND_LABEL[mode]} ${formatBytes(mine)}, free ${formatBytes(usage.free)}`;
  const text = !added
    ? lead
    : overflow
      ? `${lead}; adding ${formatBytes(added)} is ${formatBytes(overflow)} too much`
      : `${lead}; adding ${formatBytes(added)}`;
  return { segments, base, free: usage.free, added, overflow, text };
}
