import type { SetJson } from '../src/export.ts';

export interface Report {
  errors: Array<{ id: string; text: string }>;
  warnings: Array<{ id: string; text: string }>;
  missing: string[];
  missingV1: string[];
}

export interface LoadError {
  id: string;
  message: string;
  where: string;
}

export interface SetPayload extends SetJson {
  report: Report;
  errors: LoadError[];
}

/** What the page shows. Held by the app object in main.ts, not at module level. */
export interface ViewState {
  set: SetPayload | null;
  scale: number;
  grid: boolean;
  inverted: boolean;
  animate: boolean;
  selected: string | null;
  /** detail panel: position of the value slider (0..1) or the shown state */
  value: number;
  state: number;
}

export const initialState = (): ViewState => ({
  set: null,
  scale: 4,
  grid: false,
  inverted: false,
  animate: true,
  selected: null,
  value: 0.5,
  state: 0,
});
