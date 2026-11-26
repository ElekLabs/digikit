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
  /** The runtime's code and text, for the tooltip. */
  detail?: string;
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

/** The runtime's own name and text of a status code, for the tooltip of a result. */
const REASONS = new Map<number, string>(
  [...STATUS, ...LOAD_STATUS].map(([code, name, text]) => [code, text ? `${name}: ${text}` : name]),
);

/** Plain sentences for the codes a live change can end with; the lead is what the user reads, the code stays in the tooltip. */
const PLAIN = new Map<number, string>([
  [-1, 'Not found: the file is not on the device.'],
  [-4, 'The file on the device is damaged. Upload it again.'],
  [-5, 'The file is not a valid module. Upload it again.'],
  [-6, 'The module needs a newer runtime than the device has.'],
  [-7, 'A service the module needs is missing. Upload the module again to add it.'],
  [-8, 'Out of memory: the module is too big for what is free.'],
  [-9, 'The module did not start (its init failed).'],
  [-10, 'Already loaded: a module of that name is running.'],
  [-12, 'Busy: a task is still using it. Try again.'],
  [-30, 'The module has DSP code, but this firmware has no DSP loader.'],
  [-33, 'The DSP area has no room for this module.'],
  [-35, 'The transfer to the DSP failed. Try again.'],
  [-36, 'The module is built for the other device.'],
  [-43, 'A card transfer is running. Try again when it ends.'],
  [-44, 'The device is in safe mode: no module can load. Restart it.'],
  [-45, 'This module can only load at boot. Restart the device.'],
  [-48, 'The module was built for another runtime version.'],
  [-73, 'A service the module needs is not loaded.'],
  [-81, 'A service file was built for another runtime. Upload it again.'],
  [-82, 'A service does not fit this firmware (other version or patched).'],
  [-87, 'The DSP loader could not release the module. Restart the device.'],
]);

/** The runtime's name and text of a status code (`E_BUSY: ...`): what the tooltip of a result shows. `-87` is the DSP loader that could not release a module. */
export function liveCode(code: number): string {
  if (code === -87) return 'E_DSP_RELEASE (-87): the DSP loader holds a module it could not release';
  return REASONS.get(code) ?? `status ${code}`;
}

/** The plain sentence for a status code; a code without one gets the runtime's text. */
export function liveReason(code: number): string {
  const plain = PLAIN.get(code);
  if (plain) return plain;
  const text = REASONS.get(code);
  return text ? `${text.replace(/^E_\w+: /, '').replace(/^./, (c) => c.toUpperCase())}.` : `Status ${code}.`;
}

/** The result of one change from the lists `liveModules` returns for its path. */
export function liveResult(op: LiveOp, path: string, out: LiveOutcome): LiveResult {
  const refused = out.refused.find((r) => r.path === path);
  if (refused)
    return {
      op,
      kind: 'refused',
      text: `${liveReason(refused.result)} Nothing changed.`,
      detail: `${LIVE_LABEL[op]} refused: ${liveCode(refused.result)}`,
    };
  const failed = out.failed.find((r) => r.path === path);
  if (failed)
    return {
      op,
      kind: 'failed',
      text: `${liveReason(failed.result)} The module is not loaded.`,
      detail: `${LIVE_LABEL[op]} failed: ${liveCode(failed.result)}`,
    };
  if (!out.done.includes(path)) return { op, kind: 'failed', text: `${LIVE_LABEL[op]} did not finish.` };
  return { op, kind: 'done', text: `${PAST[op]}.` };
}
