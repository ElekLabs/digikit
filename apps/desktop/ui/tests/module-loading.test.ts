import { expect, test, vi } from 'vitest';
import type { DkSession } from '@digi/dk-session';
import { createFakeSessions, createFakeTransferApp, type FakeTransferApp } from '../../../web/src/transfer/fake-app';

const mocks = vi.hoisted(() => ({ info: vi.fn(), scan: vi.fn(), buildView: vi.fn() }));
vi.mock('../../../web/src/modules/dktransfer', async (original) => ({
  ...(await original<object>()),
  buildView: mocks.buildView,
}));

/** An app whose emulator session is a stub (INFO from `mocks.info`) and whose library is `mocks.scan`. */
function setup(): FakeTransferApp {
  mocks.scan.mockReset().mockResolvedValue([]);
  mocks.buildView.mockReset().mockResolvedValue({ device: 'dn2' });
  return createFakeTransferApp({
    sessions: createFakeSessions(
      () => ({ info: mocks.info, rpc: async () => new Uint8Array() }) as unknown as DkSession,
    ),
    env: {
      library: { scan: mocks.scan, read: async () => new Uint8Array() },
      targets: {
        emulator: true,
        ports: async () => [],
        open: vi.fn(async () => ({ send: () => {}, recv: async () => null })),
      },
    },
  });
}

test('local modules appear before the device answers, and the library is read once', async () => {
  const { backend } = setup();
  let answer!: (info: object) => void;
  mocks.info.mockReturnValue(new Promise((resolve) => (answer = resolve)));
  let shown!: () => void;
  const visible = new Promise<void>((resolve) => (shown = resolve));
  const show = vi.fn(() => shown());
  const read = backend.list('dn2', show);
  await visible;
  expect(mocks.buildView).not.toHaveBeenCalled();
  expect(show.mock.calls).toHaveLength(1);
  answer({ port: 'dn2', runtime: 0 });
  await read;
  expect(mocks.scan.mock.calls).toEqual([['dn2']]);
});

test('the local library remains available when the device fails', async () => {
  const { backend } = setup();
  mocks.info.mockRejectedValue(new Error('device unavailable'));
  const show = vi.fn();
  await expect(backend.list('dn2', show)).rejects.toThrow('device unavailable');
  expect(show).toHaveBeenCalledOnce();
  expect(mocks.buildView).not.toHaveBeenCalled();
});

test('a different connected device uses its own library for the final view', async () => {
  const { backend } = setup();
  mocks.info.mockResolvedValue({ port: 'dn2', runtime: 0 });
  await backend.list('dt2', () => {});
  expect(mocks.scan.mock.calls).toEqual([['dt2'], ['dn2']]);
  expect(mocks.buildView).toHaveBeenCalledOnce();
});
