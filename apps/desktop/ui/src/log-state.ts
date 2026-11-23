// The in-memory workbench log and the selection shared by its tool drawers.
import { batch, createSignal, untrack } from 'solid-js';
import type { KeyValue } from '@digi/workbench-core/storage.ts';
import {
  addEntries,
  autoOpens,
  clearEntries,
  emptyLog,
  unseenErrors,
  type LogEntry,
  type LogInput,
  type LogLevel,
  type LogSource,
} from './log-core';

const FOLLOW_KEY = 'digikit.logFollow';

export type DrawerName = 'Log' | 'Analyzer' | 'Performance';

export interface DrawerSelection {
  active(): DrawerName | undefined;
  select(name: DrawerName | undefined): void;
  toggle(name: DrawerName): void;
}

export interface LogReader {
  entries(): LogEntry[];
  unread(): number;
  followOn(): boolean;
  setFollowOn(on: boolean): void;
  clear(): void;
}

export interface LogWriter {
  logInput(input: LogInput): void;
  logInputs(inputs: LogInput[]): void;
  logEvent(source: LogSource, level: LogLevel, text: string, detail?: string): void;
}

export function createLogDrawer(storage: KeyValue): { drawer: DrawerSelection; log: LogReader; logWriter: LogWriter } {
  const [active, setActive] = createSignal<DrawerName>();
  const [state, setState] = createSignal(emptyLog());
  const [seenId, setSeenId] = createSignal(0);
  const [followOn, setFollow] = createSignal(storage.get(FOLLOW_KEY) !== 'off');

  const select = (name: DrawerName | undefined) =>
    batch(() => {
      setActive(name);
      if (name === 'Log') setSeenId(untrack(state).nextId - 1);
    });

  const logInputs = (inputs: LogInput[]) =>
    batch(() => {
      setState((current) => addEntries(current, inputs, Date.now()));
      if (untrack(active) === 'Log' || inputs.some(autoOpens)) select('Log');
    });
  const logInput = (input: LogInput) => logInputs([input]);

  return {
    drawer: {
      active,
      select,
      toggle: (name) => select(untrack(active) === name ? undefined : name),
    },
    log: {
      entries: () => state().entries,
      unread: () => (active() === 'Log' ? 0 : unseenErrors(state().entries, seenId())),
      followOn,
      setFollowOn(on) {
        setFollow(on);
        storage.set(FOLLOW_KEY, on ? 'on' : 'off');
      },
      clear: () => void setState(clearEntries),
    },
    logWriter: {
      logInput,
      logInputs,
      logEvent: (source, level, text, detail) => logInput({ source, level, text, ...(detail ? { detail } : {}) }),
    },
  };
}
