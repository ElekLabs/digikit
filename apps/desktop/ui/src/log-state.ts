// The workbench log: one list for the window, written by every part of the app (`logEvent`) and shown by the Log drawer in Console.tsx.
// In memory only; the rules are in log-core.ts.
import { createSignal } from 'solid-js';
import {
  addEntries,
  addEntry,
  autoOpens,
  clearEntries,
  emptyLog,
  type LogInput,
  type LogLevel,
  type LogSource,
} from './log-core';

export const [logState, setLogState] = createSignal(emptyLog());
/** The drawer is open. An error that `autoOpens` sets it. */
export const [logOpen, setLogOpen] = createSignal(false);

export function logInput(input: LogInput): void {
  setLogState((s) => addEntry(s, input, Date.now()));
  if (autoOpens(input)) setLogOpen(true);
}
/** Several entries received together (a poll of a runtime log). */
export function logInputs(inputs: LogInput[]): void {
  setLogState((s) => addEntries(s, inputs, Date.now()));
  if (inputs.some(autoOpens)) setLogOpen(true);
}
export const logEvent = (source: LogSource, level: LogLevel, text: string, detail?: string): void =>
  logInput({ source, level, text, ...(detail ? { detail } : {}) });
export const clearLog = (): void => {
  setLogState(clearEntries);
};

// Follow: read the runtime log of the emulator and the device. On unless the user turned it off; kept per browser profile of the app.
const FOLLOW_KEY = 'digikit.logFollow';
const readFollow = (): boolean => {
  try {
    return localStorage.getItem(FOLLOW_KEY) !== 'off';
  } catch {
    return true;
  }
};
export const [followOn, setFollowSignal] = createSignal(readFollow());
export function setFollowOn(on: boolean): void {
  setFollowSignal(on);
  try {
    localStorage.setItem(FOLLOW_KEY, on ? 'on' : 'off');
  } catch {
    /* the choice lasts until the window closes */
  }
}
