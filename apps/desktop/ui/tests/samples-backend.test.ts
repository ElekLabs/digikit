// The desktop Samples backend over a FakeHost (apps/desktop/ui/src/samples-backend.ts): what it asks of the host and what it shows.
import assert from 'node:assert/strict';
import { beforeEach, test, vi } from 'vitest';
import { RPC_PING, RPC_SOFTWARE_VERSION } from '@digi/dk-session';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import { createFakeSessions, createFakeTransferApp } from '@digi/workbench-core/transfer/fake-app.ts';
import { createFakeHost } from '@digi/workbench-core/host/fake-host.ts';
import { createSamplesBackend } from '../src/samples-backend.ts';
import {
  localEntries,
  localKinds,
  setLocalEntries,
  setLocalFolder,
  setLocalPath,
  setLocalKinds,
} from '../src/samples-state.ts';

// the converter is the wasm of dksample: not needed to open the backend
vi.mock('@digi/dk-session/dksample', async (original) => ({
  ...(await original<typeof import('@digi/dk-session/dksample')>()),
  Sample: { create: async () => ({ hash: () => new Uint8Array(32) }) },
  converterOf: () => undefined,
}));

const WAV = new TextEncoder().encode('RIFF\0\0\0\0WAVEfmt ');
const listing = [
  { name: 'kick.wav', kind: 'file' as const, size: 4000 },
  { name: 'notes.txt', kind: 'file' as const, size: 12 },
  { name: 'drums', kind: 'folder' as const, size: 0 },
];

beforeEach(() => {
  setLocalFolder('');
  setLocalPath('');
  setLocalEntries(undefined);
  setLocalKinds({});
});

test('reading the folder lists the open level, then labels the files from their first 64 bytes', async () => {
  const host = createFakeHost({
    'fs.list': listing,
    'fs.read': (_root: string, rel: string) => (rel === 'kick.wav' ? WAV.buffer : new Uint8Array(4).buffer),
  });
  setLocalFolder('/samples');
  await createSamplesBackend(host, createFakeTransferApp()).readLocal();
  assert.deepEqual(localEntries(), listing);
  assert.deepEqual(host.argsOf('fs.list'), [['/samples', '']]);
  // folders are not labelled; each file is read with head 64
  assert.deepEqual(host.argsOf('fs.read').sort(), [
    ['/samples', 'kick.wav', 64],
    ['/samples', 'notes.txt', 64],
  ]);
  assert.equal(localKinds()['kick.wav'], 'converted');
  assert.equal(localKinds()['notes.txt'], 'unchanged');
});

test('a folder that cannot be listed shows an empty list and reports the error', async () => {
  const host = createFakeHost({
    'fs.list': () => {
      throw new Error('permission denied');
    },
  });
  setLocalFolder('/samples');
  await assert.rejects(createSamplesBackend(host, createFakeTransferApp()).readLocal(), /permission denied/);
  assert.deepEqual(localEntries(), []);
});

test('no folder chosen: nothing is asked of the host', async () => {
  const host = createFakeHost();
  const backend = createSamplesBackend(host, createFakeTransferApp());
  await backend.readLocal();
  assert.equal(localEntries(), undefined);
  assert.equal(await backend.listLocalNow(), undefined);
  assert.deepEqual(host.calls, []);
});

test('the light poll lists the open level and answers undefined when that fails', async () => {
  let ok = true;
  const host = createFakeHost({
    'fs.list': () => {
      if (!ok) throw new Error('gone');
      return listing;
    },
  });
  setLocalFolder('/samples');
  setLocalPath('drums');
  const backend = createSamplesBackend(host, createFakeTransferApp());
  assert.deepEqual(await backend.listLocalNow(), listing);
  assert.deepEqual(host.argsOf('fs.list'), [['/samples', 'drums']]);
  ok = false;
  assert.equal(await backend.listLocalNow(), undefined);
});

test('a stock device is identified by Ping: no runtime, SysEx link, and no runtime request is made', async () => {
  const device =
    registry.devices.find((d) => d.storage?.samples === 'stock-rpc') ?? registry.devices.find((d) => d.storage)!;
  const calls: string[] = [];
  const text = (...parts: string[]) => Uint8Array.from(Buffer.from(parts.map((p) => `${p}\0`).join('')));
  const session = {
    rpc: async (type: number) => {
      calls.push(`rpc:${type}`);
      if (type === RPC_PING) return Uint8Array.from([2, 1, 2, ...text(device.name, 'rev')]);
      if (type === RPC_SOFTWARE_VERSION) return text('00', '79', '9.9');
      throw new Error(`unexpected rpc ${type}`);
    },
    // a stock device does not answer INFO
    info: async () => {
      calls.push('info');
      throw Object.assign(new Error('no reply from the device'), { kind: 'timeout' });
    },
    loadFailures: async () => void calls.push('loadFailures'),
    usbFastWrongMode: async () => void calls.push('usbFastWrongMode'),
  };
  const host = createFakeHost({ 'firmware.sampleWasm': new ArrayBuffer(0) });
  const app = createFakeTransferApp({ sessions: createFakeSessions(() => session) });
  const opened = await createSamplesBackend(host, app).openDeviceSamples();
  assert.equal(opened.name, device.name);
  assert.equal(opened.hasRuntime, false);
  assert.equal(opened.runtime, null);
  assert.equal(opened.link, 'sysex');
  // the one INFO is the identification's probe; nothing else of the runtime is asked
  assert.deepEqual(
    calls.filter((c) => !c.startsWith('rpc:')),
    ['info'],
  );
});

test('the folder dialog starts at the given folder, or nowhere', async () => {
  const host = createFakeHost({ 'fs.pickFolder': '/picked' });
  const backend = createSamplesBackend(host, createFakeTransferApp());
  assert.equal(await backend.pickFolder('/old'), '/picked');
  await backend.pickFolder('');
  assert.deepEqual(
    host.argsOf('fs.pickFolder').map(([o]) => (o as { start: string | null }).start),
    ['/old', null],
  );
});
