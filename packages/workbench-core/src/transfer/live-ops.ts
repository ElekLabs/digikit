// Live changes of the modules on the device (no boot), as the Transfer page offers them: which action a row gets, and what its result says.
// The change itself is `liveModules` (src/modules/dktransfer.ts), the function `wb transfer load|unload|reload` calls too.
import { LOAD_STATUS, STATUS } from '@digi/dk-session/gen_proto';
import type { LiveOp, ReloadOutcome, TransferRow } from '../modules/dktransfer.ts';
import type { CardServiceRef } from '../modules/dktransfer.ts';

export type { LiveOp };

/** What a live change ended with, for the row: `done`, `refused` (nothing changed) or `failed` (the old module was torn down; it is not loaded). */
export interface LiveResult {
  op: LiveOp;
  kind: 'done' | 'refused' | 'failed';
  text: string;
}

/** The outcome lists of `liveModules` for one path. */
export interface LiveOutcome {
  done: string[];
  refused: ReloadOutcome[];
  failed: ReloadOutcome[];
}

export const LIVE_LABEL: Record<LiveOp, string> = { load: 'Load', unload: 'Unload', reload: 'Reload' };
const PAST: Record<LiveOp, string> = { load: 'Loaded', unload: 'Unloaded', reload: 'Reloaded' };

/** The actions of a module row on the card: a loaded module reloads or unloads, one that is on the card and not loaded loads. A row only the library has, a service file and a port file get none. */
export function liveActions(r: Pick<TransferRow, 'path' | 'loaded' | 'on_card'>): LiveOp[] {
  if (r.on_card === 'no' || !r.path.endsWith('.dkm')) return [];
  return r.loaded ? ['reload', 'unload'] : ['load'];
}

/** The action of a service of the dependency popover: only a service on the card reloads (a service is never unloaded here). */
export function serviceAction(s: CardServiceRef): { op: 'reload'; path: string } | undefined {
  return s.state === 'on device' ? { op: 'reload', path: s.path } : undefined;
}

const REASONS = new Map<number, string>(
  [...STATUS, ...LOAD_STATUS].map(([code, name, text]) => [code, text ? `${name}: ${text}` : name]),
);

/** The runtime's reason for a status code (its own table: name and text); `-87` is the DSP loader that could not release a module. */
export function liveReason(code: number): string {
  if (code === -87) return 'the DSP loader holds a module it could not release; restart the device';
  return REASONS.get(code) ?? `status ${code}`;
}

/** The result of one change from the lists `liveModules` returns for its path. */
export function liveResult(op: LiveOp, path: string, out: LiveOutcome): LiveResult {
  const refused = out.refused.find((r) => r.path === path);
  if (refused)
    return { op, kind: 'refused', text: `${LIVE_LABEL[op]} refused: ${liveReason(refused.result)}. Nothing changed.` };
  const failed = out.failed.find((r) => r.path === path);
  if (failed)
    return {
      op,
      kind: 'failed',
      text: `${LIVE_LABEL[op]} failed: ${liveReason(failed.result)}. The module is not loaded.`,
    };
  if (!out.done.includes(path)) return { op, kind: 'failed', text: `${LIVE_LABEL[op]} did not finish.` };
  return { op, kind: 'done', text: `${PAST[op]}.` };
}
