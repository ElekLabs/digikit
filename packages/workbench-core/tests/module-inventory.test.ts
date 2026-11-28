// The module inventory: which read may answer (packages/workbench-core/src/transfer/module-inventory.ts).
import { describe, expect, it, vi } from 'vitest';
import { createRoot } from 'solid-js';

const read = vi.hoisted(() => ({ next: undefined as undefined | (() => Promise<unknown[]>) }));
vi.mock('../src/modules/dktransfer.ts', () => ({ readLoadedModules: () => read.next!() }));

import { createModuleInventory } from '../src/transfer/module-inventory.ts';

const opened = async () => ({ session: {} as never, info: {} as never });
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((a, b) => ((resolve = a), (reject = b)));
  return { promise, resolve, reject };
}

describe('module inventory', () => {
  it('a late failure of an older refresh keeps the list a newer refresh filled', async () => {
    const inv = createRoot(createModuleInventory);
    const a = deferred<unknown[]>();
    read.next = () => a.promise;
    const first = inv.refresh(opened);
    await new Promise((r) => setTimeout(r, 0)); // the first read has started and waits on `a`
    read.next = async () => [{ name: 'pm' }];
    await inv.refresh(opened);
    expect(inv.modules()).toEqual([{ name: 'pm' }]);
    a.reject(new Error('late'));
    await first;
    expect(inv.modules()).toEqual([{ name: 'pm' }]);
  });

  it('a late failure of open() of an older refresh is dropped too', async () => {
    const inv = createRoot(createModuleInventory);
    const a = deferred<never>();
    const first = inv.refresh(() => a.promise);
    read.next = async () => [{ name: 'pm' }];
    await inv.refresh(opened);
    a.reject(new Error('late'));
    await first;
    expect(inv.modules()).toEqual([{ name: 'pm' }]);
  });

  it('a target change during open discards the result', async () => {
    const inv = createRoot(createModuleInventory);
    const o = deferred<Awaited<ReturnType<typeof opened>>>();
    read.next = async () => [{ name: 'old' }];
    const p = inv.refresh(() => o.promise);
    inv.clear();
    o.resolve(await opened());
    await p;
    expect(inv.modules()).toEqual([]);
    expect(inv.settled()).toBe(false);
  });
});
