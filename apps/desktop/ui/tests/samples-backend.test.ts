import assert from 'node:assert/strict';
import { test, vi } from 'vitest';
import { fileHashHex, RPC_PING, RPC_SOFTWARE_VERSION } from '@digi/dk-session';
import type { DkEntry } from '@digi/dk-session';
import { FakeBulkDevice } from '@digi/dk-session/bulk-fake';
import { RPC_FILE } from '@digi/dk-session/rpcfiles';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import {
  createFakeSessions,
  createFakeTransferApp,
  silentTransport,
  type FakeTransferApp,
} from '@digi/workbench-core/transfer/fake-app.ts';
import { createFakeHost } from '@digi/workbench-core/host/fake-host.ts';
import { runTransfer } from '@digi/workbench-core/transfer/run-transfer.ts';
import { memoryKeyValue } from '@digi/workbench-core/storage.ts';
import type { TransferBulk, TransferRun, TransferTargets } from '@digi/workbench-core/transfer/env.ts';
import { createSamplesApp } from '../src/samples/app.ts';
import { SAMPLE_FOLDER_KEY } from '../src/samples/source.ts';

vi.mock('@digi/dk-session/dksample', async (original) => ({
  ...(await original<typeof import('@digi/dk-session/dksample')>()),
  Sample: { create: async () => ({ hash: () => 0 }) },
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((finish, fail) => {
    resolve = finish;
    reject = fail;
  });
  return { promise, resolve, reject };
}

const writer = { logEvent: () => {} };
const WAV = new TextEncoder().encode('RIFF\0\0\0\0WAVEfmt ');
const listing = [
  { name: 'kick.wav', kind: 'file' as const, size: 4000 },
  { name: 'notes.txt', kind: 'file' as const, size: 12 },
  { name: 'drums', kind: 'folder' as const, size: 0 },
];

function windowWith(
  host: ReturnType<typeof createFakeHost>,
  {
    folder = '/samples',
    sessions = createFakeSessions(),
    logRun = (_run: TransferRun) => {},
    onJobEnd,
    bulk,
    openTarget = async () => silentTransport(),
  }: {
    folder?: string;
    sessions?: ReturnType<typeof createFakeSessions>;
    logRun?: (run: TransferRun) => void;
    onJobEnd?: () => void;
    bulk?: TransferBulk;
    openTarget?: TransferTargets['open'];
  } = {},
) {
  const storage = memoryKeyValue({ [SAMPLE_FOLDER_KEY]: folder, 'digikit.transferMode': 'samples' });
  let app!: FakeTransferApp;
  const samples = createSamplesApp({
    fs: host.fs,
    firmware: host.firmware,
    storage,
    getTarget: () => app.target,
    getState: () => app.state,
    getConnection: () => app.connection,
    getBulk: () => app.env.bulk,
    run: (what, job) => runTransfer(app, what, job),
    writer,
  });
  app = createFakeTransferApp({
    storage,
    sessions,
    env: {
      extras: { samples: { ...samples.transfer, Mode: () => undefined }, onJobEnd },
      targets: { emulator: true, ports: async () => [], open: openTarget },
      logRun,
      bulk,
    },
  });
  return { app, samples, storage };
}

test('reading a folder lists the open level and labels files from their first 64 bytes', async () => {
  const host = createFakeHost({
    'fs.list': listing,
    'fs.read': (_root: string, rel: string) => (rel === 'kick.wav' ? WAV.buffer : new Uint8Array(4).buffer),
  });
  const { source } = windowWith(host).samples;
  await source.read();
  assert.deepEqual(source.entries(), listing);
  assert.deepEqual(host.argsOf('fs.list'), [['/samples', '']]);
  assert.deepEqual(host.argsOf('fs.read').sort(), [
    ['/samples', 'kick.wav', 64],
    ['/samples', 'notes.txt', 64],
  ]);
  assert.equal(source.rows().find((row) => row.name === 'kick.wav')?.sample, 'converted');
  assert.equal(source.rows().find((row) => row.name === 'notes.txt')?.sample, 'unchanged');
});

test('a folder that cannot be listed shows an empty list and reports the error', async () => {
  const host = createFakeHost({
    'fs.list': () => {
      throw new Error('permission denied');
    },
  });
  const { source } = windowWith(host).samples;
  await assert.rejects(source.read(), /permission denied/);
  assert.deepEqual(source.entries(), []);
});

test('with no folder selected, neither the read nor poll asks the host', async () => {
  const host = createFakeHost();
  const { source } = windowWith(host, { folder: '' }).samples;
  await source.read();
  assert.equal(source.entries(), undefined);
  assert.equal(await source.changed(), false);
  assert.deepEqual(host.calls, []);
});

test('the light poll compares the open level and keeps the current listing if the folder is temporarily unavailable', async () => {
  let entries = listing;
  let ok = true;
  const host = createFakeHost({
    'fs.list': () => {
      if (!ok) throw new Error('gone');
      return entries;
    },
    'fs.read': new ArrayBuffer(0),
  });
  const { source } = windowWith(host).samples;
  await source.open('drums');
  assert.equal(await source.changed(), false);
  entries = [];
  assert.equal(await source.changed(), true);
  ok = false;
  assert.equal(await source.changed(), false);
  assert.deepEqual(
    host.argsOf('fs.list'),
    Array.from({ length: 4 }, () => ['/samples', 'drums']),
  );
});

const stock = registry.devices.find((d) => d.storage?.samples === 'stock-rpc')!;
const store = registry.devices.find((d) => d.storage?.samples === 'digikit')!;
const text = (...parts: string[]) => new TextEncoder().encode(parts.map((p) => p + '\0').join(''));
const ping = (name: string) => Uint8Array.of(2, 1, 2, ...text(name, 'rev'));

test('a stock device is identified by Ping and uses SysEx without asking the runtime for USB diagnostics', async () => {
  const calls: string[] = [];
  const session = {
    async rpc(type: number) {
      calls.push(`rpc:${type}`);
      if (type === RPC_PING) return ping(stock.name);
      if (type === RPC_SOFTWARE_VERSION) return text('00', '79', '9.9');
      if (type === RPC_FILE.READ_DIR) return new Uint8Array();
      throw new Error(`unexpected rpc ${type}`);
    },
    async info() {
      calls.push('info');
      throw Object.assign(new Error('no reply from the device'), { kind: 'timeout' });
    },
  };
  const host = createFakeHost({ 'firmware.sampleWasm': new ArrayBuffer(0) });
  const { samples, app } = windowWith(host, { folder: '', sessions: createFakeSessions(() => session) });
  await samples.transfer.read();
  assert.deepEqual(app.state.deviceInfo(), {
    name: stock.name,
    runtime: null,
    hasRuntime: false,
    samples: 'stock-rpc',
    canMkdir: true,
    apply: 'live',
    free: undefined,
  });
  assert.deepEqual(app.state.sampleLink(), { state: 'sysex' });
  assert.deepEqual(
    calls.filter((c) => !c.startsWith('rpc:')),
    ['info'],
  );
});

test('the folder dialog starts at the remembered folder, or nowhere', async () => {
  const host = createFakeHost({ 'fs.pickFolder': '/picked', 'fs.list': [] });
  const a = windowWith(host, { folder: '/old' });
  const b = windowWith(host, { folder: '' });
  await a.samples.source.choose();
  await b.samples.source.choose();
  assert.deepEqual(host.argsOf('fs.pickFolder'), [[{ start: '/old' }], [{ start: null }]]);
  assert.equal(a.storage.get(SAMPLE_FOLDER_KEY), '/picked');
});

test('source folder, rows, added files and selection belong to the window and survive its target change', async () => {
  const host = createFakeHost({ 'fs.list': listing, 'fs.read': new ArrayBuffer(0) });
  const a = windowWith(host, { folder: '/mine' });
  const b = windowWith(host, { folder: '/theirs' });
  await a.samples.source.open('drums');
  a.samples.source.add([new File(['clip'], 'external.txt')]);
  await a.app.target.selectTarget({ kind: 'midi', port: 'Other port' });
  assert.equal(a.samples.source.folder(), '/mine');
  assert.equal(a.samples.source.path(), 'drums');
  assert.deepEqual(
    a.samples.source.picked().map((row) => row.name),
    ['external.txt'],
  );
  assert.equal(b.samples.source.folder(), '/theirs');
  assert.equal(b.samples.source.path(), '');
  assert.deepEqual(b.samples.source.rows(), []);
  assert.equal(b.samples.source.selected().size, 0);
});

function storeSession(list: (prefix?: string) => Promise<DkEntry[]>) {
  return {
    async rpc(type: number) {
      if (type === RPC_PING) return ping(store.name);
      if (type === RPC_SOFTWARE_VERSION) return text('00', '79', '9.9');
      throw new Error(`unexpected rpc ${type}`);
    },
    info: async () => ({ port: store.id, runtime: 1, caps: 0, free_bytes: 4096 }),
    usbFastWrongMode: async () => false,
    list,
  };
}

function entry(name: string): DkEntry {
  return { path: `samples/${name}`, size: 40, hash: 'file hash' };
}

test.each(['reply', 'error'] as const)(
  'a delayed listing %s cannot publish after selecting A, B, then A',
  async (outcome) => {
    const started = deferred<void>();
    const first = deferred<DkEntry[]>();
    let openings = 0;
    const sessions = createFakeSessions(() => {
      const index = openings++;
      return storeSession(async () => {
        if (index !== 0) return [entry(index === 1 ? 'from-b' : 'from-new-a')];
        started.resolve();
        return first.promise;
      });
    });
    const host = createFakeHost({ 'firmware.sampleWasm': new ArrayBuffer(0) });
    const { samples, app } = windowWith(host, { folder: '', sessions });
    const a = { kind: 'midi', port: 'A' } as const;
    await app.target.selectTarget(a);
    const old = samples.transfer.read();
    await started.promise;
    await app.target.selectTarget({ kind: 'midi', port: 'B' });
    await samples.transfer.read();
    assert.deepEqual(
      samples.destination.entries()?.map((row) => row.name),
      ['from-b'],
    );
    await app.target.selectTarget(a);
    await samples.transfer.read();
    if (outcome === 'reply') first.resolve([entry('from-old-a')]);
    else first.reject(new Error('the old A went away'));
    await old;
    assert.deepEqual(
      samples.destination.entries()?.map((row) => row.name),
      ['from-new-a'],
    );
    assert.equal(app.state.error(), '');
  },
);

test('only a pending read that lost its target binding reports an interruption', async () => {
  const started = deferred<void>();
  const listing = deferred<DkEntry[]>();
  const sessions = createFakeSessions(() =>
    storeSession(() => {
      started.resolve();
      return listing.promise;
    }),
  );
  const host = createFakeHost({ 'firmware.sampleWasm': new ArrayBuffer(0) });
  const { samples, app } = windowWith(host, {
    folder: '',
    sessions,
    openTarget: async (target) => {
      if (target.kind === 'midi' && target.port === 'Unavailable') throw new Error('Port gone');
      return silentTransport();
    },
  });
  const { read, readInterrupted } = samples.transfer;
  await app.target.selectTarget({ kind: 'midi', port: 'A' });
  const inFlight = read();
  await started.promise;
  assert.equal(readInterrupted(), false);
  await assert.rejects(app.target.selectTarget({ kind: 'midi', port: 'Unavailable' }), /Port gone/);
  assert.deepEqual(app.target.target(), { kind: 'midi', port: 'A' });
  assert.equal(readInterrupted(), true);
  listing.resolve([]);
  await inFlight;
  assert.equal(readInterrupted(), false);
});

test.each([false, true])(
  'a stale Transfer job reads the new Samples target once (read already queued: %s)',
  async (queued) => {
    const aStarted = deferred<void>();
    const aListing = deferred<DkEntry[]>();
    const bFinished = deferred<void>();
    const reads: string[] = [];
    const sessions = createFakeSessions((key) =>
      storeSession(async () => {
        reads.push(key);
        if (key === 'midi:B') return [entry('from-b')];
        aStarted.resolve();
        return aListing.promise;
      }),
    );
    const host = createFakeHost({ 'firmware.sampleWasm': new ArrayBuffer(0) });
    const { samples, app } = windowWith(host, {
      folder: '',
      sessions,
      onJobEnd() {
        if (!app.state.busy()) bFinished.resolve();
      },
    });
    await app.target.selectTarget({ kind: 'midi', port: 'A' });
    const old = runTransfer(app, 'Reading', async () => {});
    await aStarted.promise;
    await app.target.selectTarget({ kind: 'midi', port: 'B' });
    if (queued) await runTransfer(app, 'Reading', async () => {});
    aListing.resolve([entry('from-a')]);
    await old;
    await bFinished.promise;
    // A is listed once; B once for the folder and once more for the storage bar (a store without folders lists everything for both)
    assert.deepEqual(reads, ['midi:A', 'midi:B', 'midi:B']);
    assert.deepEqual(
      samples.destination.entries()?.map((row) => row.name),
      ['from-b'],
    );
  },
);

test('a chosen local sample uploads through the shared Transfer job and appears in its refreshed device listing', async () => {
  const bytes = new TextEncoder().encode('the sample fixture');
  const begun = deferred<void>();
  const finish = deferred<void>();
  const files = new Map<string, Uint8Array>();
  const listings: string[][] = [];
  const runs: TransferRun[] = [];
  const sessions = createFakeSessions(() => ({
    ...storeSession(async () => {
      listings.push([...files.keys()]);
      return [...files].map(([path, data]) => ({ path, size: data.length, hash: fileHashHex(data) }));
    }),
    async put(path: string, data: Uint8Array, progress: (sent: number, total: number) => void) {
      assert.equal(path, 'samples/hit.raw');
      assert.deepEqual(data, bytes);
      progress(4, data.length);
      begun.resolve();
      await finish.promise;
      files.set(path, data);
      progress(data.length, data.length);
    },
    async verify(path: string) {
      return fileHashHex(files.get(path)!);
    },
  }));
  const host = createFakeHost({
    'firmware.sampleWasm': new ArrayBuffer(0),
    'fs.list': [{ name: 'hit.raw', kind: 'file', size: bytes.length }],
    'fs.read': bytes.buffer,
  });
  const { samples, app } = windowWith(host, { sessions, logRun: (run) => runs.push(run) });
  await app.target.selectTarget({ kind: 'midi', port: 'Samples fixture' });
  await runTransfer(app, 'Reading', async () => {});
  assert.deepEqual(samples.destination.entries(), []);
  const row = samples.source.rows()[0]!;
  samples.source.select(new Set([row.key]));
  const upload = samples.uploadRows();
  await begun.promise;
  const job = app.jobs.currentJob();
  assert.equal(job?.label, 'Uploading');
  assert.ok(job.progress && job.progress > 0 && job.progress < 1);
  assert.equal(samples.destination.bar(true)?.sent, 4);
  finish.resolve();
  await upload;
  assert.deepEqual(
    samples.destination.entries()?.map((row) => row.name),
    ['hit.raw'],
  );
  assert.equal(samples.destination.rows()[0]?.status?.kind, 'ok');
  assert.equal(app.state.busy(), '');
  assert.equal(samples.source.selected().size, 0);
  assert.deepEqual(listings.at(-1), ['samples/hit.raw']);
  assert.equal(runs[0]?.target, 'MIDI: Samples fixture');
  assert.match(runs[0]?.lines.join('\n') ?? '', /hit.raw.*verified/);
});

test('target selection closes a late bulk claim before the next target can claim the interface', async () => {
  const first = new FakeBulkDevice();
  const second = new FakeBulkDevice();
  const started = deferred<void>();
  const finish = deferred<FakeBulkDevice>();
  const opens: string[] = [];
  let lateClose = false;
  const bulk: TransferBulk = {
    candidates: async () => [{ id: 'bulk interface' }],
    async open() {
      if (!opens.length) {
        opens.push('first');
        started.resolve();
        return finish.promise;
      }
      opens.push('second');
      assert.equal(first.closed, true);
      return second;
    },
  };
  const sessions = createFakeSessions((key) => ({
    ...storeSession(async () => []),
    async usbFastNonce(nonce: Uint8Array) {
      (key === 'emulator' ? first : second).setNonce(nonce);
    },
  }));
  const host = createFakeHost({ 'firmware.sampleWasm': new ArrayBuffer(0) });
  const { samples, app } = windowWith(host, { folder: '', sessions, bulk });
  const old = samples.transfer.read();
  await started.promise;
  const switching = app.target.selectTarget({ kind: 'midi', port: 'Other port' }).then(() => {
    lateClose = first.closed;
  });
  assert.equal(app.target.switching(), true);
  finish.resolve(first);
  await switching;
  await old;
  assert.equal(lateClose, true);
  await samples.transfer.read();
  assert.deepEqual(opens, ['first', 'second']);
  assert.equal(second.closed, false);
  assert.deepEqual(app.state.sampleLink(), { state: 'bulk' });
});

test('target selection can finish during the old MIDI nonce request without allowing a late claim', async () => {
  const started = deferred<void>();
  const finish = deferred<void>();
  const device = new FakeBulkDevice();
  let claims = 0;
  const bulk: TransferBulk = {
    candidates: async () => [{ id: 'bulk interface' }],
    async open() {
      claims++;
      return device;
    },
  };
  const sessions = createFakeSessions((key) => ({
    ...storeSession(async () => []),
    async usbFastNonce(nonce: Uint8Array) {
      if (key === 'midi:A') {
        started.resolve();
        await finish.promise;
        return;
      }
      device.setNonce(nonce);
    },
  }));
  const host = createFakeHost({ 'firmware.sampleWasm': new ArrayBuffer(0) });
  const { samples, app } = windowWith(host, { folder: '', sessions, bulk });
  await app.target.selectTarget({ kind: 'midi', port: 'A' });
  const old = samples.transfer.read();
  await started.promise;
  await app.target.selectTarget({ kind: 'midi', port: 'B' });
  await samples.transfer.read();
  assert.equal(claims, 1);
  finish.resolve();
  await old;
  assert.equal(claims, 1);
  assert.equal(device.closed, false);
  assert.deepEqual(app.state.sampleLink(), { state: 'bulk' });
});

test('an upload planned for an earlier selection cannot start after A, B, then A', async () => {
  const walk = deferred<typeof listing>();
  const started = deferred<void>();
  const host = createFakeHost({
    'firmware.sampleWasm': new ArrayBuffer(0),
    'fs.list': (_root: string, path: string) => {
      if (!path) return listing;
      started.resolve();
      return walk.promise;
    },
    'fs.read': new ArrayBuffer(0),
  });
  const writes: string[] = [];
  const sessions = createFakeSessions(() => ({
    ...storeSession(async () => []),
    put: async (path: string) => void writes.push(path),
  }));
  const { samples, app } = windowWith(host, { sessions });
  const a = { kind: 'midi', port: 'A' } as const;
  await app.target.selectTarget(a);
  await samples.source.read();
  samples.source.select(new Set(['drums']));
  const upload = samples.uploadRows();
  await started.promise;
  await app.target.selectTarget({ kind: 'midi', port: 'B' });
  await app.target.selectTarget(a);
  walk.resolve([{ name: 'new-file', kind: 'file', size: 0 }]);
  await upload;
  assert.deepEqual(writes, []);
  assert.equal(app.sessions.made.length, 0);
  assert.equal(app.state.error(), '');
  assert.deepEqual([...samples.source.selected()], ['drums']);
});

test('a failed port change keeps the open folder and lets it delete its selected file immediately', async () => {
  const host = createFakeHost({ 'firmware.sampleWasm': new ArrayBuffer(0) });
  const firstRead = deferred<void>();
  const calls: string[] = [];
  const path = 'samples/drums/nested';
  let present = true;
  const sessions = createFakeSessions((key) => ({
    ...storeSession(async () => {
      calls.push(`list ${key}`);
      return present ? [{ ...entry('nested'), path }] : [];
    }),
    async delete(name: string) {
      calls.push(`delete ${key} ${name}`);
      present = false;
    },
  }));
  const { samples, app } = windowWith(host, {
    folder: '',
    sessions,
    onJobEnd: () => firstRead.resolve(),
    async openTarget(target) {
      if (target.kind === 'midi' && target.port === 'Unavailable') throw new Error('Port unavailable');
      return silentTransport();
    },
  });
  const a = { kind: 'midi', port: 'A' } as const;
  await app.target.selectTarget(a);
  samples.openDestination('drums');
  await firstRead.promise;
  samples.destination.select(new Set(['drums/nested']));
  const info = app.state.deviceInfo();
  assert.ok(info);
  await assert.rejects(app.target.selectTarget({ kind: 'midi', port: 'Unavailable' }), /Port unavailable/);
  assert.deepEqual(app.target.target(), a);
  assert.equal(samples.destination.folder(), 'drums');
  assert.deepEqual(app.state.deviceInfo(), info);
  assert.equal(app.state.sampleLink(), undefined);
  assert.deepEqual(
    samples.destination.entries()?.map((row) => row.name),
    ['nested'],
  );
  await samples.removeSelected();
  // every read lists the folder and then the store for the storage bar
  assert.deepEqual(calls, ['list midi:A', 'list midi:A', `delete midi:A ${path}`, 'list midi:A', 'list midi:A']);
  assert.deepEqual(samples.destination.entries(), []);
});
