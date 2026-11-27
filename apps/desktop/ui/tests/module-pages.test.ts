import { batch, createRoot, createSignal } from 'solid-js';
import { describe, expect, it, vi } from 'vitest';
import type { DkModule } from '@digi/dk-session';
import {
  createMemoryModuleChannel,
  definePage,
  type DescribedSchema,
  type ModuleChannel,
  type ModulePageDefinition,
} from '@digi/module-ui';
import type { ModuleChannelFactory } from '@digi/workbench-core/module-ui/module-channel-factory.ts';
import type { ModulePage } from '@digi/workbench-core/module-ui/module-page.ts';
import { createModulePageRegistry } from '@digi/workbench-core/module-ui/module-page-registry.ts';
import { schema as sample } from '../../../../tests/fixtures/modules/settings-a/workbench/gen/settings';
import { runtimeModulePages, type ModulePagesHost } from '../src/runtime-module-pages';

const Page = () => null;
const record = (name: string, uiInstance?: number) =>
  ({ name, index: 1, ...(uiInstance ? { uiInstance } : {}) }) as DkModule;

function harness(options: { definition?: ModulePageDefinition; gate?: Promise<void> } = {}) {
  const [target, setTarget] = createSignal<{ kind: 'midi'; port: string }>({ kind: 'midi', port: 'A' });
  const [modules, setModules] = createSignal<DkModule[]>([]);
  const host = {
    target: { target, switching: () => false, libraryOnly: () => false },
    inventory: { modules },
  } as unknown as ModulePagesHost;
  const requests: string[] = [];
  const channels: ModuleChannelFactory = ({ target: key, current }) => {
    requests.push(key);
    const channel = createMemoryModuleChannel(sample, { current }).channel;
    return { ...channel, describeSettings: async () => (await options.gate, channel.describeSettings()) };
  };
  const registry = createModulePageRegistry({
    '/test/workbench/index.tsx': async () => ({ default: options.definition }),
  });
  const built: { id: string; definition: ModulePageDefinition | undefined; channel: ModuleChannel }[] = [];
  let lookup!: (module: DkModule) => ModulePage | undefined;
  const dispose = createRoot((dispose) => {
    lookup = runtimeModulePages(host, channels, registry, (id, schema: DescribedSchema, definition, channel) => {
      built.push({ id, definition, channel: channel(() => true) });
      return {
        descriptor: { id, name: schema.name, schema, hasPage: definition !== undefined, hasFiles: false },
        open: () => ({}),
      };
    });
    return dispose;
  });
  return { setTarget, setModules, lookup, built, requests, dispose };
}

const settled = () => new Promise((resolve) => setTimeout(resolve, 0));
const page = definePage({ id: sample.module, api: 1, Page });

describe('the pages of the modules loaded on the selected target', () => {
  it('gives a loaded sample one page that carries the page it ships, named from its schema', async () => {
    const h = harness({ definition: page });
    h.setModules([record(sample.module, 7)]);
    await vi.waitFor(() => expect(h.lookup(record(sample.module, 7))).toBeDefined());
    expect(h.lookup(record(sample.module, 7))?.descriptor.name).toBe('Boot settings');
    expect(h.built).toHaveLength(1);
    expect(h.built[0]).toMatchObject({ id: sample.module, definition: page });
    h.dispose();
  });
  it('gives a module on the card that is not loaded, or has no instance, no page', async () => {
    const h = harness({ definition: page });
    h.setModules([record(sample.module)]);
    await settled();
    expect(h.lookup(record(sample.module))).toBeUndefined();
    expect(h.built).toEqual([]);
    h.dispose();
  });
  it('leaves the generic form when the page is missing or of another api version', async () => {
    for (const definition of [undefined, { ...page, api: 2 } as unknown as ModulePageDefinition]) {
      const h = harness({ definition });
      h.setModules([record(sample.module, 3)]);
      await vi.waitFor(() => expect(h.built).toHaveLength(1));
      expect(h.built[0]!.definition).toBeUndefined();
      expect(h.lookup(record(sample.module, 3))).toBeDefined();
      h.dispose();
    }
  });
  it('drops the page when the module unloads and builds a new one for a new instance', async () => {
    const h = harness({ definition: page });
    h.setModules([record(sample.module, 1)]);
    await vi.waitFor(() => expect(h.lookup(record(sample.module, 1))).toBeDefined());
    h.setModules([]);
    expect(h.lookup(record(sample.module, 1))).toBeUndefined();
    h.setModules([record(sample.module, 2)]);
    await vi.waitFor(() => expect(h.lookup(record(sample.module, 2))).toBeDefined());
    expect(h.lookup(record(sample.module, 1))).toBeUndefined();
    expect(h.built).toHaveLength(2);
    h.dispose();
  });
  it('lets a late result of the old target reach neither the new target nor its channel', async () => {
    let release!: () => void;
    const h = harness({ definition: page, gate: new Promise<void>((resolve) => (release = resolve)) });
    h.setModules([record(sample.module, 1)]);
    await settled();
    batch(() => {
      h.setTarget({ kind: 'midi', port: 'B' });
      h.setModules([]);
    });
    release();
    await settled();
    expect(h.built).toEqual([]);
    expect(h.lookup(record(sample.module, 1))).toBeUndefined();
    expect(h.requests).toEqual(['midi:A']);
    h.dispose();
  });
});
