// The groups of both Transfer panes and the service files behind them. "On your computer" and "On device" list the same groups (Machines, Effects,
// LFO shapes, Tools); the device adds "Not in library" for a module only the card has. A service file is never a row: it shows in the
// dependency popover of the modules that need it, and a service no module on the card needs is counted in the pane header (`unusedServices`).
import type { Group } from '../modules/module-path.ts';
import type { TransferRow, TransferView } from '../modules/dktransfer.ts';

export const GROUPS: Group[] = ['Machines', 'Effects', 'LFO shapes', 'Tools'];
/** The extra group of the device pane: a card-only module that no library module matches. */
export const NOT_IN_LIBRARY = 'Not in library';
export type PaneGroup = Group | typeof NOT_IN_LIBRARY;

export interface RowGroup<R extends TransferRow = TransferRow> {
  group: PaneGroup;
  rows: R[];
}

/** The one grouping of both panes. Every group of `GROUPS` is returned (empty ones too); with `notInLibrary` the rows without a library id are
 * taken out of their header group into a last group, which is returned only when it has rows. */
export function groupRows<R extends TransferRow>(rows: R[], opts: { notInLibrary?: boolean } = {}): RowGroup<R>[] {
  const lost = opts.notInLibrary ? rows.filter((r) => r.id === null) : [];
  const out: RowGroup<R>[] = GROUPS.map((group) => ({
    group,
    rows: rows.filter((r) => r.group === group && !lost.includes(r)),
  }));
  if (lost.length) out.push({ group: NOT_IN_LIBRARY, rows: lost });
  return out;
}

/** A service file of the card (`.dks`; port files are no services). */
export const isServiceRow = (r: Pick<TransferRow, 'path'>): boolean => r.path.endsWith('.dks');
/** A port file of the card (`.dkport`). */
export const isPortRow = (r: Pick<TransferRow, 'path'>): boolean => r.path.endsWith('.dkport');
/** A system file (service or port file): never a module row. */
export const isSystemRow = (r: Pick<TransferRow, 'path'>): boolean => isServiceRow(r) || isPortRow(r);

/** The port file of the card as the pane header shows it: `ok` (built for the connected firmware), `other` (only files for another firmware), `missing`. */
export interface PortLine {
  state: 'ok' | 'other' | 'missing';
  text: string;
  /** The files for another firmware: they serve no module and can be ticked for Delete. */
  stale: string[];
}

const portStem = (path: string): string => (path.split('/').pop() ?? path).replace(/\.dkport$/, '');

/** The port line of the device pane header, from the port files in the card inventory. `missing` is a warning only when a module on the card needs a service (a card with none needs no port file). */
export function portLine(view: TransferView): PortLine | undefined {
  const files = [...view.modules.filter((r) => r.on_card !== 'no'), ...view.card_only].filter(isPortRow);
  const stale = files.filter((r) => r.for_firmware !== undefined).map((r) => r.path);
  const good = files.find((r) => r.for_firmware === undefined);
  if (good) return { state: 'ok', text: `port ${portStem(good.path)} ✓`, stale };
  if (files.length)
    return {
      state: 'other',
      text: `port file is for another firmware (${files.map((r) => portStem(r.path)).join(', ')})`,
      stale,
    };
  const needs = [...view.modules, ...view.card_only].some((r) => r.on_card !== 'no' && r.card_services?.length);
  return needs ? { state: 'missing', text: 'port file missing: services cannot run', stale } : undefined;
}

/** The services that no module on the card needs. `verified` is false when some card file could not be read, so a service may be needed after all. */
export interface UnusedServices {
  paths: string[];
  verified: boolean;
}

/** The service files on the card that no module needs: not in the services of a module on the card, and no file names them in `needed_by`. */
export function unusedServices(view: TransferView): UnusedServices {
  const rows = [...view.modules.filter((r) => r.on_card !== 'no'), ...view.card_only];
  const attached = new Set(rows.flatMap((r) => (r.card_services ?? []).map((s) => s.path)));
  const paths = rows
    .filter((r) => isServiceRow(r) && !attached.has(r.path) && !r.needed_by?.length)
    .map((r) => r.path)
    .sort();
  return { paths, verified: !view.incomplete_card_needs };
}

/** The text of the pane header warning, or '' when every service is needed. */
export function unusedServicesText(u: UnusedServices): string {
  const n = u.paths.length;
  if (!n) return '';
  const files = `${n} ${n === 1 ? 'service file' : 'service files'}`;
  return u.verified
    ? `${files} no module needs`
    : `${files} no module is known to need (some card files could not be read)`;
}
