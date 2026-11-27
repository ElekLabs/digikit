import { describe, expect, it, vi } from 'vitest';
import { definePage } from '@digi/module-ui';
import { createModulePageRegistry } from '../src/module-ui/module-page-registry.ts';

const Page = () => null;
const page = (id: string, api = 1) => ({ default: { id, api, Page } });

describe('the module page registry', () => {
  it('finds a page by its module id among lazy loaders and loads each file once', async () => {
    const loadA = vi.fn(async () => page('test/a'));
    const loadB = vi.fn(async () => ({ default: definePage({ id: 'test/b', api: 1, Page }) }));
    const registry = createModulePageRegistry({ '/a/workbench/index.tsx': loadA, '/b/workbench/index.tsx': loadB });
    expect((await registry.find('test/b'))?.id).toBe('test/b');
    expect((await registry.find('test/a'))?.id).toBe('test/a');
    expect(loadA).toHaveBeenCalledTimes(1);
    expect(loadB).toHaveBeenCalledTimes(1);
  });
  it('finds nothing for a module without a page, so the generic form shows', async () => {
    const registry = createModulePageRegistry({ '/a/workbench/index.tsx': async () => page('test/a') });
    expect(await registry.find('test/other')).toBeUndefined();
    expect(await createModulePageRegistry({}).find('test/a')).toBeUndefined();
  });
  it('ignores a page of another api version, a file with no page and a file that fails to load', async () => {
    const registry = createModulePageRegistry({
      '/old/workbench/index.tsx': async () => page('test/a', 2),
      '/none/workbench/index.tsx': async () => ({}),
      '/broken/workbench/index.tsx': async () => {
        throw new Error('syntax');
      },
      '/ok/workbench/index.tsx': async () => page('test/ok'),
    });
    expect(await registry.find('test/a')).toBeUndefined();
    expect((await registry.find('test/ok'))?.id).toBe('test/ok');
  });
  it('tries a file that failed to load again at the next lookup', async () => {
    let fail = true;
    const registry = createModulePageRegistry({
      '/a/workbench/index.tsx': async () => {
        if (fail) throw new Error('not yet');
        return page('test/a');
      },
    });
    expect(await registry.find('test/a')).toBeUndefined();
    fail = false;
    expect((await registry.find('test/a'))?.id).toBe('test/a');
  });
  it('lets a failing or slow loader hide no other page, and logs the failure once', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const registry = createModulePageRegistry({
      '/bad/workbench/index.tsx': async () => {
        throw new Error('syntax');
      },
      '/slow/workbench/index.tsx': () => new Promise(() => {}),
      '/ok/workbench/index.tsx': async () => page('test/ok'),
    });
    expect((await registry.find('test/ok'))?.id).toBe('test/ok');
    expect((await registry.find('test/ok'))?.id).toBe('test/ok');
    expect(warn).toHaveBeenCalledTimes(1);
    warn.mockRestore();
  });
});
