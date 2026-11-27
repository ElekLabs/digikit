// The Card log button of the Log drawer, the pure part: what the button shows for the state the debug module reports. No DOM and no timers.
// The state comes from LOGCFG op 2 (log-follow.ts); host tests: apps/desktop/ui/tests/log-cardlog.test.ts.
import type { CardLogState } from '@digi/dk-session';

export const NO_CARD_LOG = 'This debug module has no card log';
const OFF_TITLE = 'Write the device log to the card (data/digikit/debug). Kept across restarts.';

export interface CardLogView {
  label: string;
  title: string;
  /** The log is switched on (also when it stopped on a full store or an error). */
  pressed: boolean;
  disabled: boolean;
  /** The dot after the label (writing, or stopped). */
  dot: boolean;
  /** The log stopped: full store or error. */
  warn: boolean;
}

/**
 * @param state what the origin reported: null = no card log, undefined = not read yet
 * @param off why the log controls are off (`levelsOff`), or ''
 */
export function cardLogView(state: CardLogState | null | undefined, off: string): CardLogView {
  const label = 'Card log';
  if (off) return { label, title: off, pressed: false, disabled: true, dot: false, warn: false };
  if (state === null) return { label, title: NO_CARD_LOG, pressed: false, disabled: true, dot: false, warn: false };
  if (state === undefined)
    return { label, title: 'Reading the card log state…', pressed: false, disabled: true, dot: false, warn: false };
  if (state.state === 'full')
    return { label, title: 'Card log stopped: store full', pressed: true, disabled: false, dot: true, warn: true };
  if (state.state === 'error')
    return {
      label,
      title: `Card log stopped: error ${state.lastStatus}`,
      pressed: true,
      disabled: false,
      dot: true,
      warn: true,
    };
  if (!state.on) return { label, title: OFF_TITLE, pressed: false, disabled: false, dot: false, warn: false };
  const last = state.msSinceWrite === null ? 'no write yet' : `last ${Math.round(state.msSinceWrite / 1000)} s ago`;
  return {
    label,
    title: `Writing to cardlog-${state.file}.bin: ${state.writes} writes, ${last}`,
    pressed: true,
    disabled: false,
    dot: true,
    warn: false,
  };
}
