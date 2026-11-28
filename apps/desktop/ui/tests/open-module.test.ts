import { createRoot, createSignal } from 'solid-js';
import { describe, expect, it, vi } from 'vitest';
import type { DkModule } from '@digi/dk-session';
import { createModuleInventory } from '@digi/workbench-core/transfer/module-inventory.ts';
import { watchOpenModule } from '../src/shell/open-module';

const read = vi.hoisted(() => ({ next: undefined as undefined | (() => Promise<unknown[]>) }));
vi.mock('@digi/workbench-core/modules/dktransfer.ts', () => ({ readLoadedModules: () => read.next!() }));

const pm = { name: 'project-manager', index: 1, uiInstance: 3 } as DkModule;
const later = () => new Promise((resolve) => setTimeout(resolve, 0));

function harness() {
  const inventory = createModuleInventory();
  const [target, setTarget] = createSignal('A');
  const left = vi.fn();
  createRoot(() =>
    watchOpenModule(
      { target, switching: () => false, inventory, hasPage: (m) => m.uiInstance !== undefined },
      () => ({ id: 'project-manager' }),
      left,
    ),
  );
  const run = (modules: DkModule[], gate?: Promise<void>) => {
    read.next = async () => (await gate, await later(), modules);
    return inventory.reader()({} as never, {} as never);
  };
  return { inventory, setTarget, left, run };
}

describe('the module open on the page', () => {
  it('stays through an inventory clear and refill', async () => {
    const h = harness();
    await h.run([pm]);
    h.inventory.clear();
    await later();
    let release!: () => void;
    const refill = h.run([pm], new Promise<void>((resolve) => (release = resolve)));
    await later();
    expect(h.left).not.toHaveBeenCalled();
    release();
    await refill;
    expect(h.left).not.toHaveBeenCalled();
  });

  it('stays while a refresh runs over a listed module', async () => {
    const h = harness();
    await h.run([pm]);
    let release!: () => void;
    const refresh = h.run([], new Promise<void>((resolve) => (release = resolve)));
    await later();
    expect(h.left).not.toHaveBeenCalled();
    release();
    await refresh;
    expect(h.left).toHaveBeenCalledTimes(1);
  });

  it('leaves when a finished read no longer lists the module', async () => {
    const h = harness();
    await h.run([pm]);
    await h.run([{ ...pm, name: 'other' }]);
    expect(h.left).toHaveBeenCalledTimes(1);
  });

  it('leaves when the target changes', async () => {
    const h = harness();
    await h.run([pm]);
    h.setTarget('B');
    expect(h.left).toHaveBeenCalledTimes(1);
  });
});
