import { createEffect, createSignal, onCleanup } from 'solid-js';
import type { DkModule, ModuleSettingsEndpoint } from '@digi/dk-session';
import type { DescribedSchema } from '@digi/module-ui';
import { describeDeviceSettings } from '@digi/workbench-core/module-ui/device-settings.ts';
import type { ModulePage } from '@digi/workbench-core/module-ui/module-page.ts';
import type { TransferApp } from '@digi/workbench-core/transfer/app.ts';

// Keep a resolved page for each loaded instance on this selection. A new
// inventory read alone does not remount open controls or discard edits.
export function runtimeModulePages(
  app: TransferApp,
  pageFor: (
    id: string,
    schema: DescribedSchema,
    open: (mounted: () => boolean) => ModuleSettingsEndpoint,
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
    const binding = app.target.bindSession();
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
      const current = () => entries.get(name) === entry && app.target.target() === target && !app.target.switching();
      void binding
        .session()
        .then(async (session) => {
          if (!current()) return;
          const schema = await describeDeviceSettings(session.moduleSettings(index, instance, current));
          if (!current()) return;
          entry.page = pageFor(name, schema, (mounted) =>
            session.moduleSettings(index, instance, () => current() && mounted()),
          );
          publish();
        })
        .catch(() => {
          // A future inventory refresh can retry; existing file controls stay available.
          if (current()) entries.delete(name);
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
