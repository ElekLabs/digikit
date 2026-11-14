// The Transfer app over fakes (apps/web/src/transfer/app.ts): the target and its sessions, and what the app keeps in its KeyValue.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { createDefaultModules } from '../src/firmware/default-modules-state.ts';
import type { FirmwareEnv } from '../src/firmware/env.ts';
import { memoryKeyValue, remembered } from '../src/storage.ts';
import { createFakeSessions, createFakeTransferApp, silentTransport } from '../src/transfer/fake-app.ts';
import type { TransferEnv } from '../src/transfer/env.ts';

const opened: string[] = [];
const closed: string[] = [];
const targets = (): TransferEnv['targets'] => ({
  emulator: true,
  ports: async () => ['Digitakt II'],
  async open(t) {
    opened.push(t.kind === 'midi' ? t.port : t.kind);
    return silentTransport();
  },
  async close(t) {
    closed.push(t.kind === 'midi' ? t.port : t.kind);
  },
});

test('the target starts on the emulator, or on the MIDI port kept in the storage', () => {
  assert.deepEqual(createFakeTransferApp().target.target(), { kind: 'emulator' });
  const storage = memoryKeyValue({ 'digikit.target': JSON.stringify({ kind: 'midi', port: 'Digitone II' }) });
  const app = createFakeTransferApp({ storage });
  assert.deepEqual(app.target.target(), { kind: 'midi', port: 'Digitone II' });
  assert.equal(app.target.lastMidiPort(), 'Digitone II');
  // a stored choice that is not a target is no choice
  const bad = createFakeTransferApp({ storage: memoryKeyValue({ 'digikit.target': '{"kind":"midi"}' }) });
  assert.deepEqual(bad.target.target(), { kind: 'emulator' });
});

test('selecting a MIDI port opens it once, keeps the choice, and the session is the one of that port', async () => {
  opened.length = closed.length = 0;
  const sessions = createFakeSessions((key) => ({ key }));
  const app = createFakeTransferApp({ env: { targets: targets() }, sessions });
  await app.target.selectTarget({ kind: 'midi', port: 'Digitakt II' });
  assert.deepEqual(opened, ['Digitakt II']);
  assert.deepEqual(app.target.target(), { kind: 'midi', port: 'Digitakt II' });
  assert.equal(app.storage.data.get('digikit.lastMidi'), 'Digitakt II');
  assert.equal(app.storage.data.get('digikit.target'), JSON.stringify({ kind: 'midi', port: 'Digitakt II' }));
  const first = await app.target.session();
  assert.equal(await app.target.session(), first, 'one session per target');
  assert.deepEqual(sessions.made, ['midi:Digitakt II']);
  // the emulator's session is another one, whatever the target is
  const emulator = (await app.target.emulatorSession()) as unknown as { key: string };
  assert.equal(emulator.key, 'emulator');
  assert.notEqual(emulator as unknown, first);
  // the same target again does nothing
  await app.target.selectTarget({ kind: 'midi', port: 'Digitakt II' });
  assert.deepEqual(opened, ['Digitakt II', 'emulator']);
});

test('leaving a MIDI port closes it, cancels its requests and drops its session; the choice is kept', async () => {
  opened.length = closed.length = 0;
  const sessions = createFakeSessions();
  const app = createFakeTransferApp({ env: { targets: targets() }, sessions });
  await app.target.selectTarget({ kind: 'midi', port: 'Digitakt II' });
  await app.target.session();
  await app.target.selectTarget({ kind: 'emulator' });
  assert.deepEqual(closed, ['Digitakt II']);
  assert.deepEqual(sessions.cancelled, ['emulator', 'midi:Digitakt II'], 'each change cancels the requests of the target it leaves');
  assert.deepEqual(sessions.dropped, ['midi:Digitakt II']);
  assert.equal(app.storage.data.get('digikit.target'), JSON.stringify({ kind: 'emulator' }));
  assert.equal(app.target.lastMidiPort(), 'Digitakt II', 'the Log still follows the last port');
});

test('a port that cannot be opened leaves the old target and keeps nothing', async () => {
  const app = createFakeTransferApp({
    env: {
      targets: {
        ...targets(),
        async open() {
          throw new Error('The MIDI port is not there.');
        },
      },
    },
  });
  await assert.rejects(app.target.selectTarget({ kind: 'midi', port: 'Gone' }), /not there/);
  assert.deepEqual(app.target.target(), { kind: 'emulator' });
  assert.equal(app.storage.data.size, 0);
});

test('a lost port is opened again by the next request', async () => {
  opened.length = 0;
  const sessions = createFakeSessions();
  const app = createFakeTransferApp({ env: { targets: targets() }, sessions });
  await app.target.selectTarget({ kind: 'midi', port: 'Digitakt II' });
  const before = await app.target.session();
  app.target.portLost('Digitakt II');
  assert.notEqual(await app.target.session(), before);
  assert.deepEqual(opened, ['Digitakt II', 'Digitakt II']);
});

test('the none target fails with the environment text, and a runtime check stops a request on an emulator without one', async () => {
  const app = createFakeTransferApp({ env: { targets: { ...targets(), whyNone: async () => 'No Web MIDI here.' } } });
  app.target.startOn({ kind: 'none' });
  await assert.rejects(app.target.session(), /No Web MIDI here/);
  app.target.startOn({ kind: 'emulator' });
  app.target.setEmulatorRuntime('no');
  assert.throws(() => app.target.checkRuntime({ kind: 'emulator' }), /without the digikit runtime/);
  app.target.setEmulatorRuntime('yes');
  app.target.checkRuntime({ kind: 'emulator' });
});

test('remembered values start from the storage and write each change; an empty value removes the key', () => {
  const storage = memoryKeyValue({ k: 'b' });
  const [value, set] = remembered(storage, 'k', (v) => (v === 'b' ? 'b' : 'a'));
  assert.equal(value(), 'b');
  set('a');
  assert.equal(storage.get('k'), 'a');
  set('' as 'a');
  assert.equal(storage.get('k'), null);
  assert.equal(value(), '');
});

test('the Transfer mode and the browsed device are remembered per storage', () => {
  const storage = memoryKeyValue({ 'digikit.transferBrowseDevice': 'dn2' });
  const app = createFakeTransferApp({ storage });
  assert.equal(app.state.browseDevice(), 'dn2');
  assert.equal(app.state.mode(), 'modules');
  app.state.setBrowseDevice('dt2');
  assert.equal(storage.data.get('digikit.transferBrowseDevice'), 'dt2');
  // the samples mode is shown only where the app has it
  app.state.setMode('samples');
  assert.equal(app.state.activeMode(), 'modules');
});

test('the default modules keep the unticked entries per device in the storage', () => {
  const env = {
    log: () => {},
    defaultModules: {
      entries: () => [{ id: 'digikit/remote', summary: 'Remote' }],
      install: async () => [],
    },
  } as unknown as FirmwareEnv;
  const storage = memoryKeyValue();
  const first = createDefaultModules(env, storage);
  assert.deepEqual(first.tickedFor('dt2'), ['digikit/remote']);
  first.setTick('dt2', 'digikit/remote', false);
  assert.deepEqual(first.tickedFor('dt2'), []);
  assert.deepEqual(first.tickedFor('dn2'), ['digikit/remote']);
  assert.deepEqual(createDefaultModules(env, storage).tickedFor('dt2'), [], 'a new app reads the kept choice');
});
