import { createRoot } from 'solid-js';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DkModule } from '@digi/dk-session';
import { createFakeSessions, createFakeTransferApp } from '@digi/workbench-core/transfer/fake-app.ts';
import { DISCONNECT_GRACE_MS } from '@digi/workbench-core/transfer/connection-state.ts';
import { watchOpenModule } from '../src/shell/open-module';
import { deviceProfiles, runtimeDeviceIds } from '@digi/workbench-core/data/device-profiles.ts';

const read = vi.hoisted(() => ({ next: undefined as undefined | (() => Promise<unknown[]>) }));
vi.mock('@digi/workbench-core/modules/dktransfer.ts', () => ({ readLoadedModules: () => read.next!() }));

const pm = { name: 'project-manager', index: 1, uiInstance: 3 } as DkModule;
const device = { id: runtimeDeviceIds[0]!, name: deviceProfiles[runtimeDeviceIds[0]!].name };
const lostReply = { type: 'lost', reason: 'no reply from the device' } as const;

// The device: `info` answers or fails, as the cable does.
function rig() {
  const state = { infoFails: false };
  const sessions = createFakeSessions(() => ({
    info: async () => {
      if (state.infoFails) throw new Error('no reply from the device');
      return {};
    },
  }));
  const app = createFakeTransferApp({ sessions });
  const left = vi.fn();
  createRoot(() =>
    watchOpenModule(
      { target: app.target.target, switching: app.target.switching, inventory: app.inventory, hasPage: () => true },
      () => ({ id: 'project-manager' }),
      left,
    ),
  );
  // what the Shell does when the connection changes: the inventory reads the target
  const refresh = async () => {
    const binding = app.target.bindSession();
    await app.inventory.refresh(async () => {
      const session = await binding.session();
      const info = await session.info(true);
      binding.check();
      return { session, info };
    });
  };
  read.next = async () => [pm];
  return { state, app, left, refresh };
}

describe('a target whose connection flaps', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('keeps the inventory and the page through a transient lost request', async () => {
    const { app, left, refresh } = rig();
    app.connection.noteConnection({ type: 'answered', device });
    await refresh();
    expect(app.inventory.modules()).toEqual([pm]);
    app.connection.noteConnection(lostReply);
    expect(app.inventory.modules()).toEqual([pm]);
    await refresh();
    app.connection.noteConnection({ type: 'answered', device });
    expect(app.inventory.modules()).toEqual([pm]);
    vi.advanceTimersByTime(DISCONNECT_GRACE_MS * 2);
    expect(app.inventory.modules()).toEqual([pm]);
    expect(left).not.toHaveBeenCalled();
  });

  it('clears the inventory and leaves the page when INFO fails after a lost request (unplugged)', async () => {
    const { state, app, left, refresh } = rig();
    app.connection.noteConnection({ type: 'answered', device });
    await refresh();
    state.infoFails = true;
    app.connection.noteConnection(lostReply);
    await refresh();
    expect(app.inventory.modules()).toEqual([]);
    expect(app.inventory.settled()).toBe(true);
    expect(left).toHaveBeenCalledTimes(1);
  });

  it('a lost request while already disconnected still starts a re-read', async () => {
    const { state, app, refresh } = rig();
    app.connection.noteConnection({ type: 'answered', device });
    await refresh();
    app.connection.noteConnection(lostReply);
    app.connection.noteConnection(lostReply);
    state.infoFails = true;
    app.connection.noteConnection(lostReply);
    await vi.advanceTimersByTimeAsync(0);
    expect(app.inventory.modules()).toEqual([]);
    expect(app.inventory.settled()).toBe(true);
  });

  it('clears the inventory when the target stays disconnected beyond the grace period', async () => {
    const { app, left, refresh } = rig();
    app.connection.noteConnection({ type: 'answered', device });
    await refresh();
    app.connection.noteConnection(lostReply);
    // the owner's re-read of the lost request answers first (the device still replies to INFO)
    await vi.advanceTimersByTimeAsync(DISCONNECT_GRACE_MS - 1);
    expect(app.inventory.modules()).toEqual([pm]);
    await vi.advanceTimersByTimeAsync(1);
    expect(app.inventory.modules()).toEqual([]);
    expect(left).toHaveBeenCalledTimes(1);
  });

  it('a restart the Workbench causes clears the inventory and the next read fills it again', async () => {
    const { app, refresh } = rig();
    app.connection.noteConnection({ type: 'answered', device });
    await refresh();
    app.connection.noteConnection({ type: 'closed' });
    expect(app.inventory.modules()).toEqual([]);
    expect(app.inventory.settled()).toBe(false);
    app.connection.noteConnection({ type: 'answered', device });
    await refresh();
    expect(app.inventory.modules()).toEqual([pm]);
    expect(app.inventory.settled()).toBe(true);
  });
});
