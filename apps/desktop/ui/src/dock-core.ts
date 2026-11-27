// The pure part of the bottom dock (WorkbenchDrawer.tsx): its height in px, kept between a minimum and a maximum that leaves room
// for the page above it. Host tests: apps/desktop/ui/tests/dock.test.ts.

export const DOCK_MIN_PX = 120;
/** The page above the dock keeps at least this much (not counting the top bar and the status bar). */
export const DOCK_KEEP_PX = 280;
/** The top bar and the status bar, in px (about; the maximum is a guide, not a layout rule). */
export const DOCK_CHROME_PX = 80;
/** The default height: this share of the window. */
export const DOCK_DEFAULT_SHARE = 0.3;
/** What an arrow key moves the dock edge by, in px. */
export const DOCK_KEY_STEP = 24;

/** The saved height, or undefined when it is not a positive number. */
export function parseDock(raw: string | null | undefined): number | undefined {
  const n = Number(raw);
  return raw && Number.isFinite(n) && n > 0 ? n : undefined;
}

/** The largest height for a window `windowPx` tall; never below the minimum. */
export const dockMax = (windowPx: number): number => Math.max(DOCK_MIN_PX, windowPx - DOCK_CHROME_PX - DOCK_KEEP_PX);

/** `height` kept between the minimum and the maximum. */
export const clampDock = (height: number, windowPx: number): number =>
  Math.round(Math.min(dockMax(windowPx), Math.max(DOCK_MIN_PX, height)));

/** The height to start with: the saved one, else the default share of the window. */
export const startDock = (saved: number | undefined, windowPx: number): number =>
  clampDock(saved ?? windowPx * DOCK_DEFAULT_SHARE, windowPx);

/** The height for a pointer at `clientY` when the dock's bottom edge is at `bottom`. */
export const dockAt = (clientY: number, bottom: number, windowPx: number): number =>
  clampDock(bottom - clientY, windowPx);
