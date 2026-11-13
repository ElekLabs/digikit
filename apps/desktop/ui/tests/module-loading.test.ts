import { expect, test, vi } from 'vitest';
import { transferBackend } from '../../../web/src/transfer/transfer-backend';
import { setTransferEnv } from '../../../web/src/transfer/env';

const mocks = vi.hoisted(() => ({ info: vi.fn(), scan: vi.fn(), buildView: vi.fn() }));
vi.mock('../../../web/src/modules/dkneeds', () => ({ DkNeeds: { create: async () => ({}) } }));
vi.mock('../../../web/src/modules/dktransfer', async (original) => ({
  ...(await original<object>()),
  buildView: mocks.buildView,
}));
vi.mock('../../../web/src/transfer/target', () => ({
  checkRuntime: async () => {},
  session: async () => ({ info: mocks.info, rpc: async () => new Uint8Array() }),
  target: () => ({ kind: 'emulator' }),
  targetLabel: () => 'Emulator',
}));

function setup() {
  mocks.scan.mockReset().mockResolvedValue([]);
  mocks.buildView.mockReset().mockResolvedValue({ device: 'dn2' });
  setTransferEnv({
    library: { scan: mocks.scan, read: async () => new Uint8Array() },
    patchkitWasm: async () => new ArrayBuffer(0),
    deviceName: async (id) => id,
    targets: { emulator: true, ports: async () => [], open: vi.fn() },
    logRun: () => {},
  });
}

test('local modules appear before the device answers, and the library is read once', async () => {
  setup();
  let answer!: (info: object) => void;
  mocks.info.mockReturnValue(new Promise((resolve) => (answer = resolve)));
  let shown!: () => void;
  const visible = new Promise<void>((resolve) => (shown = resolve));
  const show = vi.fn(() => shown());
  const read = transferBackend.list('dn2', show);
  await visible;
  expect(mocks.buildView).not.toHaveBeenCalled();
  expect(show.mock.calls).toHaveLength(1);
  answer({ port: 'dn2', runtime: 0 });
  await read;
  expect(mocks.scan.mock.calls).toEqual([['dn2']]);
});

test('the local library remains available when the device fails', async () => {
  setup();
  mocks.info.mockRejectedValue(new Error('device unavailable'));
  const show = vi.fn();
  await expect(transferBackend.list('dn2', show)).rejects.toThrow('device unavailable');
  expect(show).toHaveBeenCalledOnce();
  expect(mocks.buildView).not.toHaveBeenCalled();
});

test('a different connected device uses its own library for the final view', async () => {
  setup();
  mocks.info.mockResolvedValue({ port: 'dn2', runtime: 0 });
  await transferBackend.list('dt2', () => {});
  expect(mocks.scan.mock.calls).toEqual([['dt2'], ['dn2']]);
  expect(mocks.buildView).toHaveBeenCalledOnce();
});
