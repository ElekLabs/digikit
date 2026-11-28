// The Transfer app: everything the Transfer pane (TransferPage.tsx and the files beside it), the target selector and the Firmware page keep for
// the life of the window, built once from the app's environment and passed down (app-context.ts). There is no module-level state: a test builds
// an app over fakes (`memoryKeyValue`, a SessionHub that makes fake sessions, a TransferEnv of stubs); the desktop (main.tsx) and the site
// (web-env.ts, `createWebTransferApp`) each build one at startup.
import { createJobs, type JobsApp } from '../jobs/jobs';
import type { KeyValue } from '../storage';
import { createConnection, type ConnectionApp } from './connection-state';
import type { TransferEnv } from './env';
import { createModuleInventory } from './module-inventory';
import type { SessionHub } from './session-core';
import { createTarget, type TargetApp } from './target';
import { createTransferBackend, type BackendDeps, type TransferBackendApp } from './transfer-backend';
import { createTransferState, type TransferState } from './transfer-state';

export interface TransferApp {
  /** What the app gives the pane (the library, the targets, patchkit.wasm, the log). */
  env: TransferEnv;
  /** The app's small stored choices (the target, the mode, the folders). */
  storage: KeyValue;
  connection: ConnectionApp;
  inventory: ReturnType<typeof createModuleInventory>;
  jobs: JobsApp;
  target: TargetApp;
  /** The Transfer page's state. */
  state: TransferState;
  /** The Transfer logic over the selected target (`transferBackend`) and the built modules (`loadLibrary`). */
  backend: TransferBackendApp['transferBackend'];
  loadLibrary: TransferBackendApp['loadLibrary'];
  /** The service-aware install inputs for a device, or none when the library has no service (`transferBackend.installDeps`). */
  installDeps: TransferBackendApp['installDeps'];
}

export function createTransferApp(
  env: TransferEnv,
  deps: {
    storage: KeyValue;
    sessions: SessionHub;
    needs?: BackendDeps['needs'];
    runtimeIdentity?: BackendDeps['runtimeIdentity'];
    runtimeServices?: BackendDeps['runtimeServices'];
  },
): TransferApp {
  const { storage, sessions } = deps;
  const inventory = createModuleInventory();
  const jobs = createJobs();
  const state = createTransferState({ env, storage, jobs });
  let rereading = false;
  const connection = createConnection(
    env,
    () => {
      inventory.clear();
      state.forgetDevice();
    },
    {
      // A lost request keeps the list and re-reads it: a failed read (the device is gone) empties it, a good one confirms it.
      // The re-read asks through the session, and its own failure is a lost request again: one re-read at a time, or it would repeat without end.
      lost: () => {
        if (rereading) return;
        rereading = true;
        const binding = target.bindSession();
        void inventory
          .refresh(async () => {
            const session = await binding.session();
            const info = await session.info(true);
            binding.check();
            return { session, info };
          })
          .finally(() => (rereading = false));
      },
      expired: () => inventory.empty(),
    },
  );
  const target = createTarget({ env, storage, sessions, connection });
  const { transferBackend, loadLibrary, installDeps } = createTransferBackend({
    env,
    target,
    inventory,
    connection,
    needs: deps.needs,
    runtimeIdentity: deps.runtimeIdentity,
    runtimeServices: deps.runtimeServices,
  });
  return {
    env,
    storage,
    inventory,
    connection,
    jobs,
    target,
    state,
    backend: transferBackend,
    loadLibrary,
    installDeps,
  };
}
