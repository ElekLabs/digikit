import { describe, expect, it, vi } from 'vitest';
import { createConnection } from '@digi/workbench-core/transfer/connection-state.ts';
import { createModuleInventory } from '@digi/workbench-core/transfer/module-inventory.ts';

const read = vi.hoisted(() => ({ next: undefined as undefined | (() => Promise<unknown[]>) }));
vi.mock('@digi/workbench-core/modules/dktransfer.ts', () => ({ readLoadedModules: () => read.next!() }));

const device = { id: 'dt2', name: 'Digitakt II' };

describe('a target whose connection flaps', () => {
  it('keeps the inventory through a lost request that the next answer recovers', () => {
    const invalidate = vi.fn();
    const c = createConnection({ deviceName: async () => '' }, invalidate);
    c.noteConnection({ type: 'answered', device });
    c.noteConnection({ type: 'lost', reason: 'no reply from the device' });
    expect(c.connection().state).toBe('disconnected');
    c.noteConnection({ type: 'answered', device });
    expect(c.connection().state).toBe('connected');
    expect(invalidate).not.toHaveBeenCalled();
  });

  it('invalidates for a new target, a closed port and a missing runtime', () => {
    const invalidate = vi.fn();
    const c = createConnection({ deviceName: async () => '' }, invalidate);
    c.noteConnection({ type: 'target' });
    c.noteConnection({ type: 'closed' });
    c.noteConnection({ type: 'noruntime', reason: 'x' });
    expect(invalidate).toHaveBeenCalledTimes(3);
  });

  it('empties the list when a read fails', async () => {
    const inventory = createModuleInventory();
    read.next = async () => [{ name: 'a', index: 1 }];
    await inventory.reader()({} as never, {} as never);
    expect(inventory.modules()).toHaveLength(1);
    read.next = async () => {
      throw new Error('no reply');
    };
    await inventory.reader()({} as never, {} as never).catch(() => {});
    expect(inventory.modules()).toEqual([]);
    expect(inventory.settled()).toBe(true);
  });
});
