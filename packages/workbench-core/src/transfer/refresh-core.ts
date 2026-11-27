// When the Transfer page reads by itself (pure; apps/desktop/ui/tests/transfer-page-ui.test.ts). There is no Refresh button: the page reads when it
// opens, when the target or the mode changes, after every job, when the window gets the focus, and the folder on the computer is watched with
// a cheap listing every few seconds. Nothing reads while a job runs (a job that writes to the device is never disturbed).

/** The period of the light poll of the folder on the computer (the listing only, no file is read). */
export const POLL_MS = 3000;
/** The focus and visibility events fire together when a window comes back; one read answers both. */
export const FOCUS_GAP_MS = 800;

/** May a read that nobody asked for start now? Not while the window is hidden or any job (a read included) runs. */
export const canAutoRefresh = (visible: boolean, busy: string): boolean => visible && busy === '';

/** The window came back at `now`: read, unless a read for this came less than `FOCUS_GAP_MS` ago. */
export const focusReadDue = (lastReadAt: number | undefined, now: number): boolean =>
  lastReadAt === undefined || now - lastReadAt >= FOCUS_GAP_MS;

export interface Listed {
  name: string;
  kind: string;
  size: number;
}
/** Did the folder change? Names, kinds and sizes are compared (the order of a listing is not). An unknown listing counts as changed. */
export function listingChanged(shown: readonly Listed[] | undefined, now: readonly Listed[]): boolean {
  if (!shown || shown.length !== now.length) return true;
  const key = (e: Listed) => `${e.kind}\0${e.name}\0${e.size}`;
  const had = new Set(shown.map(key));
  return now.some((e) => !had.has(key(e)));
}
