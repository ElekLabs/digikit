// The pure part of the split pane (SplitPane.tsx): the position is a fraction of the width, kept between two minimum widths.
// Host tests: apps/web/tests/split-pane.test.ts.

export const SPLIT_DEFAULT = 0.55;
/** Width of the divider's grab area, in px (the visible line is 1 px in its middle). */
export const HANDLE_PX = 11;
/** The smallest width of a pane, in px. */
export const PANE_MIN_PX = 240;
/** What an arrow key moves the divider by (a fraction of the width). */
export const KEY_STEP = 0.02;

/** The saved position: a number strictly between 0 and 1, else the default. */
export function parseSplit(raw: string | null | undefined): number {
  const n = Number(raw);
  return raw && Number.isFinite(n) && n > 0 && n < 1 ? n : SPLIT_DEFAULT;
}

/**
 * `fraction` kept so that both panes are at least `min` px wide in a container `width` px wide (the divider takes `HANDLE_PX`). When the
 * container is too narrow for two minimum widths the position is the middle.
 */
export function clampSplit(fraction: number, width: number, min = PANE_MIN_PX): number {
  const room = width - HANDLE_PX;
  if (!(room > 0)) return SPLIT_DEFAULT;
  const lo = min / room;
  const hi = 1 - min / room;
  if (lo >= hi) return 0.5;
  return Math.min(hi, Math.max(lo, fraction));
}

/** The position for a pointer at `clientX` in a container that starts at `left` and is `width` px wide (the divider is centred on the pointer). */
export function splitAt(clientX: number, left: number, width: number, min = PANE_MIN_PX): number {
  const room = width - HANDLE_PX;
  if (!(room > 0)) return SPLIT_DEFAULT;
  return clampSplit((clientX - left - HANDLE_PX / 2) / room, width, min);
}
