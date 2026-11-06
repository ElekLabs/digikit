import assert from 'node:assert/strict';
import { test, vi } from 'vitest';

const rig = vi.hoisted(() => ({
  target: { kind: 'emulator' },
  device: vi.fn(),
  name: vi.fn<() => Promise<string>>(),
  session: { info: vi.fn(async () => ({ port: 'dn2', runtime: 0x000300 })), list: vi.fn(async () => []) },
}));
vi.mock('../../../web/src/transfer/target', () => ({ target: () => rig.target, session: async () => rig.session }));
vi.mock('../../../web/src/transfer/env', () => ({ transferEnv: () => ({ deviceName: rig.name }) }));
vi.mock('../../../web/src/transfer/transfer-state', () => ({ setDeviceInfo: rig.device }));
import { connection, noteConnection } from '../../../web/src/transfer/connection-state.ts';
import { readActiveBootscreen } from '../src/bootscreens-backend.ts';

test('a listing delayed in deviceName cannot publish connection or device state after a target change', async () => {
  let resolve!: (name: string) => void;
  let entered!: () => void;
  const named = new Promise<void>((r) => {
    entered = r;
  });
  rig.name.mockImplementation(() => {
    entered();
    return new Promise((r) => {
      resolve = r;
    });
  });
  const reading = readActiveBootscreen();
  await named;
  rig.target = { kind: 'midi' };
  resolve('Digitone II');
  await reading;
  assert.equal(connection().state, 'connecting');
  assert.equal(rig.device.mock.calls.length, 0);
});

for (const operation of ['info', 'list'] as const) {
  test(`late old ${operation.toUpperCase()} failure cannot disconnect the new target`, async () => {
    let reject!: (error: Error) => void;
    let entered!: () => void;
    const pending = new Promise<void>((r) => {
      entered = r;
    });
    rig.name.mockResolvedValue('Digitone II');
    rig.session.info.mockResolvedValue({ port: 'dn2', runtime: 0x000300 });
    rig.session.list.mockResolvedValue([]);
    rig.session[operation].mockImplementationOnce(() => {
      entered();
      return new Promise<never>((_resolve, fail) => {
        reject = fail;
      });
    });
    rig.target = { kind: 'emulator' };
    const reading = readActiveBootscreen();
    const failed = assert.rejects(reading, /old target failed/);
    await pending;
    rig.target = { kind: 'midi' };
    noteConnection({ type: 'answered', device: { id: 'dt2', name: 'new target' } });
    const newer = connection();
    reject(new Error('old target failed'));
    await failed;
    assert.equal(connection(), newer);
  });
}
