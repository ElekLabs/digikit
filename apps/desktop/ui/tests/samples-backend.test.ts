// The desktop Samples backend over a FakeHost (apps/desktop/ui/src/samples-backend.ts): what it asks of the host and what it shows.
import assert from 'node:assert/strict';
import { beforeEach, test } from 'vitest';
import { createFakeHost } from '../src/host/fake-host.ts';
import { createSamplesBackend } from '../src/samples-backend.ts';
import {
  localEntries,
  localKinds,
  setLocalEntries,
  setLocalFolder,
  setLocalPath,
  setLocalKinds,
} from '../src/samples-state.ts';

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

test('the device storage is asked once per device, and again after a failure', async () => {
  let fail = true;
  const host = createFakeHost({
    'library.deviceStorage': (device: string) => {
      if (fail) {
        fail = false;
        throw new Error('no profile');
      }
      return { name: `Device ${device}`, samples: 'midirpc' };
    },
  });
  const backend = createSamplesBackend(host);
  await assert.rejects(backend.deviceStorage('dt2'), /no profile/);
  assert.deepEqual(await backend.deviceStorage('dt2'), { name: 'Device dt2', samples: 'midirpc' });
  await backend.deviceStorage('dt2');
  await backend.deviceStorage('dn2');
  assert.deepEqual(host.argsOf('library.deviceStorage'), [['dt2'], ['dt2'], ['dn2']]);
});

test('reading the folder lists the open level, then labels the files from their first 64 bytes', async () => {
  const host = createFakeHost({
    'fs.list': listing,
    'fs.read': (_root: string, rel: string) => (rel === 'kick.wav' ? WAV.buffer : new Uint8Array(4).buffer),
  });
  setLocalFolder('/samples');
  await createSamplesBackend(host).readLocal();
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
  await assert.rejects(createSamplesBackend(host).readLocal(), /permission denied/);
  assert.deepEqual(localEntries(), []);
});

test('no folder chosen: nothing is asked of the host', async () => {
  const host = createFakeHost();
  const backend = createSamplesBackend(host);
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
  const backend = createSamplesBackend(host);
  assert.deepEqual(await backend.listLocalNow(), listing);
  assert.deepEqual(host.argsOf('fs.list'), [['/samples', 'drums']]);
  ok = false;
  assert.equal(await backend.listLocalNow(), undefined);
});

test('the folder dialog starts at the given folder, or nowhere', async () => {
  const host = createFakeHost({ 'fs.pickFolder': '/picked' });
  const backend = createSamplesBackend(host);
  assert.equal(await backend.pickFolder('/old'), '/picked');
  await backend.pickFolder('');
  assert.deepEqual(
    host.argsOf('fs.pickFolder').map(([o]) => (o as { start: string | null }).start),
    ['/old', null],
  );
});
