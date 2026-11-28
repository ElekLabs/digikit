import { createEffect, untrack } from 'solid-js';
import type { DkModule } from '@digi/dk-session';

export interface OpenModuleHost {
  /** The selected target; a different value than at open time ends the open module. */
  target(): unknown;
  switching(): boolean;
  inventory: { modules(): DkModule[]; settled(): boolean };
  /** True when the module's page can be shown (or is being built from its record). */
  hasPage(module: DkModule): boolean;
}

/**
 * Calls `leave` when the module open on the page is really gone: the target changed, or the inventory
 * finished a read that no longer lists it. An inventory that is cleared or being read keeps the page.
 * Call inside a reactive root.
 */
export function watchOpenModule(host: OpenModuleHost, open: () => { id: string } | undefined, leave: () => void) {
  let openedOn: { id: string; target: unknown } | undefined;
  createEffect(() => {
    const current = open();
    if (!current) {
      openedOn = undefined;
      return;
    }
    const target = host.target();
    if (!openedOn || openedOn.id !== current.id) openedOn = { id: current.id, target: untrack(host.target) };
    if (host.switching()) return;
    if (openedOn.target !== target) return leave();
    if (!host.inventory.settled()) return;
    const record = host.inventory.modules().find((module) => module.name === current.id);
    if (!record || !host.hasPage(record)) leave();
  });
}
