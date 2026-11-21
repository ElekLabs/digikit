import type { RigOpener } from './ops.ts';
import { round2 } from './report.ts';

export interface ConformOptions {
  /** run only these case groups (all when empty) */
  groups?: string[];
  seed?: number;
  open?: RigOpener;
}

export const SEED = 0xd1617;
export const seconds = (t0: number): number => round2((performance.now() - t0) / 1000);
