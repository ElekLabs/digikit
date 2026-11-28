import { createEffect, createSignal, onCleanup } from 'solid-js';
import type { DkModule } from '@digi/dk-session';
import type { DescribedSchema, ModuleChannel, ModulePageDefinition } from '@digi/module-ui';
import type { ModuleChannelFactory } from '@digi/workbench-core/module-ui/module-channel-factory.ts';
import type { ModulePage } from '@digi/workbench-core/module-ui/module-page.ts';
import type { ModulePageRegistry } from '@digi/workbench-core/module-ui/module-page-registry.ts';
import type { TransferApp } from '@digi/workbench-core/transfer/app.ts';
import { targetKey } from '@digi/workbench-core/transfer/session-core.ts';

/** The part of the Transfer app that decides which pages exist: the selected target and its loaded inventory. */
export interface ModulePagesHost {
  target: Pick<TransferApp['target'], 'target' | 'switching' | 'libraryOnly'>;
  inventory: { modules(): DkModule[] };
}

/** The page of a module whose page could not be built: its name and the reason, never a silent return to another page. */
export function failedPage(name: string, error: unknown): ModulePage {
  const message = `The page of ${name} could not be built: ${error instanceof Error ? error.message : String(error)}`;
  return {
    descriptor: { id: name, name, schema: { version: 1, settings: [] }, hasPage: true, hasFiles: false },
    open: () => ({ error: message }),
  };
}

// Keep a resolved page for each loaded instance on this selection. A new
// inventory read alone does not remount open controls or discard edits.
export function runtimeModulePages(
  app: ModulePagesHost,
  channels: ModuleChannelFactory,
  registry: ModulePageRegistry,
  pageFor: (
    id: string,
    schema: DescribedSchema,
    definition: ModulePageDefinition | undefined,
    channel: (current: () => boolean) => ModuleChannel,
  ) => ModulePage,
) {
  type Entry = { slot: number; instance: number; page?: ModulePage };
  let previousTarget = app.target.target();
  let entries = new Map<string, Entry>();
  const [pages, setPages] = createSignal(new Map<string, Entry>());
  function publish() {
    setPages(new Map(entries));
  }
  createEffect(() => {
    const target = app.target.target();
    const records = app.target.switching() || app.target.libraryOnly() ? [] : app.inventory.modules();
    const previous = target === previousTarget ? entries : new Map<string, Entry>();
    previousTarget = target;
    entries = new Map();
    const key = targetKey(target);
    for (const record of records) {
      const instance = record.uiInstance;
      if (!instance) continue;
      const { name, index } = record;
      const cached = previous.get(name);
      if (cached?.instance === instance && cached.slot === index) {
        entries.set(name, cached);
        continue;
      }
      const entry: Entry = { slot: index, instance };
      entries.set(name, entry);
      // Ends with the entry (unload, reload, new inventory), the target and the switch in progress: a late result then reaches nothing.
      const current = () => entries.get(name) === entry && app.target.target() === target && !app.target.switching();
      const channelFor = (mounted: () => boolean) =>
        channels({ target: key, module: record, current: () => current() && mounted() });
      void (async () => {
        const schema = await channelFor(() => true).describeSettings();
        if (!current()) return;
        const definition = await registry.find(name);
        if (!current()) return;
        entry.page = pageFor(name, schema, definition, channelFor);
        publish();
      })().catch((error: unknown) => {
        // The module keeps its entry and shows why its page could not be built; a new instance retries.
        if (!current()) return;
        entry.page = failedPage(name, error);
        publish();
      });
    }
    publish();
  });
  onCleanup(() => entries.clear());
  return (module: DkModule) => {
    const entry = pages().get(module.name);
    return entry && entry.instance === module.uiInstance && entry.slot === module.index ? entry.page : undefined;
  };
}
