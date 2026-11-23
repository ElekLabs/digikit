import { expect, it } from 'vitest';
import { createFakeTransferApp } from '@digi/workbench-core/transfer/fake-app.ts';
import { readTransfer } from '@digi/workbench-core/transfer/read-transfer.ts';

it('browses the local library without opening a stopped emulator session', async () => {
  const scanned: string[] = [];
  const app = createFakeTransferApp({
    env: {
      library: {
        scan: async (device) => {
          scanned.push(device);
          return [];
        },
        read: async () => new Uint8Array(),
      },
    },
  });
  app.target.setEmulatorAvailable(false);
  await readTransfer(app);
  expect(scanned).toEqual([app.state.browseDevice()]);
  expect(app.sessions.made).toEqual([]);
  expect(app.state.browsing()).toBe(true);
  expect(app.state.lastView()?.card_only).toEqual([]);
  expect(app.state.deviceInfo()).toBeUndefined();
});
