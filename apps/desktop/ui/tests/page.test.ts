import { expect, it } from 'vitest';
import { CAP } from '@digi/dk-session/gen_proto';
import { createFakeSessions, createFakeTransferApp } from '@digi/workbench-core/transfer/fake-app.ts';
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

it('shows the card modules while reading service details, even when the provisional library is still building', async () => {
  let finishProvisional!: (files: []) => void;
  const provisionalLibrary = new Promise<[]>((resolve) => {
    finishProvisional = resolve;
  });
  let infoRequests = 0;
  let showedModulesBeforeServices = false;
  const app = createFakeTransferApp({
    sessions: createFakeSessions(() => ({
      info: async () => {
        infoRequests++;
        return { port: 'dn2', runtime: 4, free_bytes: 1024, caps: (1 << CAP.FOLDERS) | (1 << CAP.MODULES) };
      },
      rpc: async () => new Uint8Array(),
      list: async (prefix: string) => {
        if (prefix === 'modules/') return [{ path: 'modules/example.dkm', size: 10, hash: '' }];
        if (prefix === 'services/') {
          showedModulesBeforeServices =
            !app.state.browsing() &&
            app.state.lastView()?.card_only[0]?.path === 'modules/example.dkm' &&
            app.inventory.modules()[0]?.name === 'example';
        }
        return [];
      },
      modules: async () => [{ name: 'example' }],
    })),
    env: {
      library: {
        scan: async (device) => (device === 'dt2' ? provisionalLibrary : []),
        read: async () => new Uint8Array(),
      },
    },
  });
  app.state.setBrowseDevice('dt2');
  try {
    await readTransfer(app);
  } finally {
    finishProvisional([]);
  }
  expect(showedModulesBeforeServices).toBe(true);
  expect(app.state.deviceInfo()?.name).toBe('dn2');
  expect(infoRequests).toBe(1);
});
