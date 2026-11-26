// The Transfer panes' groups, the service files behind them, and the live change actions (load, unload, reload) over fakes.
import { DkError, type DkInfoFull } from '@digi/dk-session';
import { CAP } from '@digi/dk-session/gen_proto';
import { expect, test, vi } from 'vitest';
import { liveModules, type TransferRow, type TransferView } from '../src/modules/dktransfer';
import { createFakeSessions, createFakeTransferApp } from '../src/transfer/fake-app';
import { LIVE_LABEL, liveActions, liveCode, liveReason, liveResult, serviceAction } from '../src/transfer/live-ops';
import { NOT_IN_LIBRARY, groupRows, portLine, unusedServices, unusedServicesText } from '../src/transfer/pane-groups';
import { setFitReader, type Short } from '../src/modules/fit';
import { storageBar } from '../src/transfer/storage-bar';
import {
  capacityTag,
  installBytes,
  localTags,
  spaceError,
  available,
  dependencyItems,
  deletePaths,
  deviceGroups,
  deviceTags,
} from '../src/transfer/transfer-core';

const row = (over: Partial<TransferRow> & { path: string }): TransferRow => ({
  id: null,
  name: over.path,
  group: 'Tools',
  test: false,
  version: null,
  file_size: 100,
  on_card: 'yes',
  loaded: false,
  ...over,
});
const svc = (path: string, over: Partial<TransferRow> = {}) => row({ path, name: path, ...over });
const fx = { label: 'fx service', state: 'on device', path: 'services/fx.dks', version: 0x010200, size: 800 } as const;

function view(over: Partial<TransferView> = {}): TransferView {
  return {
    device: 'dn2',
    runtime: '0.31.0',
    free_bytes: 1000,
    loaded: [],
    modules: [
      row({
        id: 'a/pluck',
        path: 'modules/pluck.dkm',
        name: 'pluck',
        group: 'Machines',
        loaded: true,
        card_services: [fx],
      }),
      row({ id: 'a/chorus', path: 'modules/chorus.dkm', name: 'chorus', group: 'Effects', on_card: 'no' }),
    ],
    card_only: [
      row({ path: 'modules/orphan.dkm', name: 'orphan', group: 'Effects' }),
      svc('services/fx.dks', { needed_by: ['modules/pluck.dkm'] }),
      svc('services/old.dks', { needed_by: [] }),
    ],
    ...over,
  };
}

test('both panes use one grouping; the device adds "Not in library" for a card-only module', () => {
  const v = view();
  expect(available(v, false).map((g) => g.group)).toEqual(['Machines', 'Effects', 'LFO shapes', 'Tools']);
  const groups = deviceGroups(v, undefined);
  expect(groups.map((g) => [g.group, g.rows.map((r) => r.path)])).toEqual([
    ['Machines', ['modules/pluck.dkm']],
    [NOT_IN_LIBRARY, ['modules/orphan.dkm']],
  ]);
  // the same function with no extra group keeps a card-only module under its header group
  expect(groupRows(v.card_only.filter((r) => !r.path.endsWith('.dks')))[1]?.rows.map((r) => r.path)).toEqual([
    'modules/orphan.dkm',
  ]);
});

test('a service file is never a row; the unused one is counted and can be deleted', () => {
  const v = view();
  const rows = deviceGroups(v, undefined).flatMap((g) => g.rows);
  expect(rows.some((r) => r.path.endsWith('.dks'))).toBe(false);
  expect(unusedServices(v)).toEqual({ paths: ['services/old.dks'], verified: true });
  expect(unusedServicesText(unusedServices(v))).toBe('1 service file no module needs');
  expect(deletePaths(v, new Set(['services/fx.dks', 'services/old.dks', 'modules/orphan.dkm']))).toEqual([
    'modules/orphan.dkm',
    'services/old.dks',
  ]);
});

test('a port file is no row; the header shows it, warns when it is for another firmware or missing', () => {
  const port = (path: string, over: Partial<TransferRow> = {}) => svc(path, over);
  const ok = view({ card_only: [...view().card_only, port('ports/dn2-1.12.dkport')] });
  expect(
    deviceGroups(ok, undefined)
      .flatMap((g) => g.rows)
      .some((r) => r.path.endsWith('.dkport')),
  ).toBe(false);
  expect(portLine(ok)).toEqual({ state: 'ok', text: 'port dn2-1.12 ✓', stale: [] });
  const other = view({ card_only: [port('ports/dn2-1.11.dkport', { for_firmware: 'abcd' })] });
  expect(portLine(other)).toMatchObject({ state: 'other', stale: ['ports/dn2-1.11.dkport'] });
  expect(deletePaths(other, new Set(['ports/dn2-1.11.dkport']))).toEqual(['ports/dn2-1.11.dkport']);
  // a matching file is shown next to a stale one; only the stale one is deletable from the header
  const both = view({
    card_only: [port('ports/dn2-1.12.dkport'), port('ports/dn2-1.11.dkport', { for_firmware: 'abcd' })],
  });
  expect(portLine(both)).toMatchObject({ state: 'ok', stale: ['ports/dn2-1.11.dkport'] });
  // missing is a warning only when a module on the card needs a service
  expect(portLine(view())?.state).toBe('missing');
  expect(portLine(view({ modules: [], card_only: [] }))).toBeUndefined();
});

test('the library side carries no capacity tag; a not-loaded device row names the short resource', () => {
  const short: Short = {
    resource: 'DSP_AREA',
    need: 400,
    free: 254,
    text: '',
    brief: '',
    describe: 'DSP memory 400 KiB needed, 254 KiB free',
    describe_by: 'DSP memory 146 KiB short',
  };
  let answer: Short | null = short;
  setFitReader({ fit: () => ({ each: [answer], set: null, line: '' }) });
  const cap = { DSP_AREA: { limit: 1000, used: 746, largest: 254 } };
  const r = row({
    id: 'a/big',
    path: 'modules/big.dkm',
    needs: { DSP_AREA: 400 },
    warnings: ['DSP memory 146 KiB short'],
  });
  expect(localTags(r).map((t) => t.text)).toEqual(['on device']);
  expect(localTags(r).some((t) => /fit/.test(t.text + (t.title ?? '')))).toBe(false);
  expect(deviceTags(r, cap).map((t) => t.text)).toEqual(['DSP memory 146 KiB short']);
  expect(capacityTag(r, cap)?.text).not.toMatch(/fit/);
  expect(capacityTag({ ...r, loaded: true }, cap)).toBeUndefined();
  expect(capacityTag({ ...r, on_card: 'no' }, cap)).toBeUndefined();
  expect(capacityTag(r, null)).toBeUndefined();
  answer = null;
  expect(capacityTag(r, cap)).toBeUndefined();
});

test('the storage bar splits the kind, the others and the free space, and previews what is ticked', () => {
  const usage = {
    total: 1000,
    free: 300,
    overhead: 50,
    used: [
      { key: 'modules' as const, label: 'Modules', bytes: 200, files: 2 },
      { key: 'services' as const, label: 'Services', bytes: 100, files: 1 },
      { key: 'samples' as const, label: 'Samples', bytes: 300, files: 5 },
      { key: 'other' as const, label: 'Other', bytes: 50, files: 1 },
    ],
  };
  const m = storageBar(usage, 'modules');
  expect(m.segments.map((s) => [s.key, s.bytes])).toEqual([
    ['modules', 200],
    ['services', 100],
    ['others', 400],
    ['free', 300],
  ]);
  expect([m.base, m.added, m.overflow]).toEqual([1000, 0, 0]);
  const s = storageBar(usage, 'samples');
  expect(s.segments.map((x) => [x.key, x.bytes])).toEqual([
    ['samples', 300],
    ['others', 350 + 50],
    ['free', 300],
  ]);
  const p = storageBar(usage, 'modules', 120);
  expect(p.segments.map((x) => [x.key, x.bytes]).slice(-2)).toEqual([
    ['preview', 120],
    ['free', 180],
  ]);
  expect(p.overflow).toBe(0);
  const over = storageBar(usage, 'samples', 500);
  expect([over.overflow, over.segments.at(-1)?.bytes]).toEqual([200, 0]);
  expect(over.text).toContain('too much');
});

test('an install that the card cannot hold is refused with the sizes; services not on the card count', () => {
  const v = view({
    modules: [
      row({ id: 'a/x', path: 'modules/x.dkm', on_card: 'no', file_size: 2048 }),
      row({ id: 'a/y', path: 'modules/y.dkm', on_card: 'yes', file_size: 5000 }),
    ],
  });
  expect(installBytes(v, new Set(['a/x', 'a/y']))).toBe(2048);
  expect(spaceError(2048, 1024)).toBe('The card is too full: needs 2 KiB, 1 KiB free.');
  expect(spaceError(1024, 1024)).toBeNull();
});

test('an unreadable card file makes the unused count a hint, not a fact', () => {
  const u = unusedServices(view({ incomplete_card_needs: true }));
  expect(u.verified).toBe(false);
  expect(unusedServicesText(u)).toContain('could not be read');
});

test('a module whose service is not on the card says what it needs; the popover lists each service', () => {
  const missing = { label: 'samples service', state: 'not on device' } as const;
  const r = row({ id: 'a/x', path: 'modules/x.dkm', card_services: [fx, missing] });
  expect(deviceTags(r).map((t) => [t.text, t.tone])).toEqual([['needs samples', 'warn']]);
  expect(dependencyItems(r.card_services)).toEqual([
    { line: 'fx service · on device · 1.2 · 800 B', path: 'services/fx.dks' },
    { line: 'samples service · not on device' },
  ]);
  expect(serviceAction(fx)).toEqual({ op: 'reload', path: 'services/fx.dks' });
  expect(serviceAction(missing)).toBeUndefined();
});

test('a module row gets Reload and Unload when loaded, Load when only on the card, nothing when only in the library', () => {
  expect(liveActions({ path: 'm.dkm', loaded: true, on_card: 'yes' })).toEqual(['reload', 'unload']);
  expect(liveActions({ path: 'm.dkm', loaded: false, on_card: 'other version' })).toEqual(['load']);
  expect(liveActions({ path: 'm.dkm', loaded: false, on_card: 'no' })).toEqual([]);
  expect(liveActions({ path: 'services/fx.dks', loaded: true, on_card: 'yes' })).toEqual([]);
  expect(liveActions({ path: 'ports/dt2-1.17.dkport', loaded: false, on_card: 'yes' })).toEqual([]);
});

test('a result is done, refused with the runtime reason (nothing changed) or failed (not loaded)', () => {
  const p = 'modules/pluck.dkm';
  const none = { done: [], refused: [], failed: [] };
  expect(liveResult('reload', p, { ...none, done: [p] })).toEqual({ op: 'reload', kind: 'done', text: 'Reloaded.' });
  const refused = liveResult('reload', p, { ...none, refused: [{ path: p, result: -12 }] });
  expect(refused.kind).toBe('refused');
  expect(refused.text).toBe('Busy: a task is still using it. Try again. Nothing changed.');
  expect(refused.detail).toContain('E_BUSY');
  expect(refused.text).not.toContain('E_BUSY');
  expect(liveResult('load', p, { ...none, refused: [{ path: p, result: -45 }] }).text).toContain('Restart the device');
  const failed = liveResult('reload', p, { ...none, failed: [{ path: p, result: -8 }] });
  expect(failed).toMatchObject({ kind: 'failed' });
  expect(failed.text).toContain('not loaded');
  expect(liveReason(-87)).toContain('Restart the device');
  expect(liveReason(-999)).toBe('Status -999.');
  // a code with no plain sentence falls back to the runtime's text, never to the bare code
  expect(liveReason(-16)).toBe('The sample pool is full.');
  expect(liveCode(-12)).toBe('E_BUSY: a task is still inside the module, or the store is in use (try again)');
  expect(liveCode(-87)).toContain('-87');
  expect(LIVE_LABEL.unload).toBe('Unload');
});

const info = {
  port: 'test',
  protocol: 1,
  runtime: 0x001f00,
  runtime_version: 0x001f00,
  free_bytes: 1,
  max_chunk: 128,
  safe_mode: false,
  upload_open: false,
  caps: (1 << CAP.FOLDERS) | (1 << CAP.MODULES) | (1 << CAP.LOADS),
  card_read_only: false,
  log_sink: false,
  fault_found: false,
  card_sectors: null,
  card_state: null,
  card_guard: null,
  image_id: null,
  svc_abi: null,
  fw_id: null,
} as DkInfoFull;

function liveSession(answers: Record<string, { result: number; unloaded: boolean } | DkError>) {
  const call = (op: string) =>
    vi.fn(async (path: string) => {
      const a = answers[`${op}:${path}`] ?? { result: 0, unloaded: false };
      if (a instanceof DkError) throw a;
      return a;
    });
  return {
    info: vi.fn(async () => info),
    rpc: async () => new Uint8Array(),
    reloadAndWait: call('reload'),
    loadAndWait: call('load'),
    unloadAndWait: call('unload'),
  };
}

test('the page action runs the function the command line runs: same session calls, same outcome and events', async () => {
  const answers = { 'reload:services/fx.dks': { result: -12, unloaded: false } };
  const session = liveSession(answers);
  const app = createFakeTransferApp({ sessions: createFakeSessions(() => session) });
  const pageEvents: unknown[] = [];
  const pageOut = await app.backend.live('reload', ['modules/pluck.dkm', 'services/fx.dks'], (e) => pageEvents.push(e));
  const cliEvents: unknown[] = [];
  const cliOut = await liveModules(session as never, 'reload', ['modules/pluck.dkm', 'services/fx.dks'], (e) =>
    cliEvents.push(e),
  );
  expect(pageOut).toEqual(cliOut);
  expect(pageEvents).toEqual(cliEvents);
  expect(pageOut.refused).toEqual([{ path: 'services/fx.dks', result: -12 }]);
  expect(session.reloadAndWait).toHaveBeenCalledTimes(4);
  expect(session.loadAndWait).not.toHaveBeenCalled();
});
