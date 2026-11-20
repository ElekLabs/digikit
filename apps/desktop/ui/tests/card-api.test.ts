// What the Card panel asks of the emulator (apps/desktop/ui/src/card-api.ts), through EmuRpc and a FakeHost.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import type { CardFolder } from '../src/card-core.ts';
import { cardApi } from '../src/card-api.ts';
import { createFakeHost } from '@digi/workbench-core/host/fake-host.ts';

const folder = (path: string): CardFolder => ({
  folder: path,
  continuous: true,
  files: [],
  last_sync: null,
  behind: false,
  error: null,
});

test('choosing a folder opens the dialog at the card folder, then sets the choice through the RPC', async () => {
  const host = createFakeHost({
    'fs.pickFolder': '/Users/em/cards/dt2',
    'rpc.call': (method: string) => folder(method === 'card.folder_set' ? '/Users/em/cards/dt2' : '/old'),
  });
  const api = cardApi(host);
  const current = folder('/old/plus_drive/dt2');
  const next = await api.choose(current, 'Choose the folder of the DIGITAKT II card');
  assert.equal(next?.folder, '/Users/em/cards/dt2');
  assert.deepEqual(host.commands(), ['fs.pickFolder', 'rpc.call']);
  assert.deepEqual(host.argsOf('rpc.call'), [['card.folder_set', { dir: '/Users/em/cards/dt2' }]]);
});

test('cancelling the dialog changes nothing', async () => {
  const host = createFakeHost({ 'fs.pickFolder': null });
  assert.equal(await cardApi(host).choose(undefined, 'title'), undefined);
  assert.deepEqual(host.commands(), ['fs.pickFolder']);
});

test('use default sets no folder; sync, reset and read are card methods of the controller', async () => {
  const host = createFakeHost({ 'rpc.call': folder('/x') });
  const api = cardApi(host);
  await api.useDefault();
  await api.sync();
  await api.resetOverlay();
  await api.folder();
  assert.deepEqual(host.argsOf('rpc.call'), [
    ['card.folder_set', {}],
    ['card.sync'],
    ['card.reset_overlay'],
    ['card.folder'],
  ]);
});

test('Show in Finder goes to the shell', async () => {
  const host = createFakeHost();
  await cardApi(host).openFolder();
  assert.deepEqual(host.commands(), ['shell.openCardFolder']);
});
