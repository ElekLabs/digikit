// @vitest-environment happy-dom
import { createComponent } from 'solid-js';
import { render } from 'solid-js/web';
import { describe, expect, it, onTestFinished, vi } from 'vitest';
import {
  createMemoryModuleChannel,
  createModuleClient,
  memoryQueryService,
  type MemoryQueryService,
} from '@digi/module-ui';
import page from '../workbench/index';
import { schema } from '../workbench/preview';
import { ProjectManager } from '../workbench/gen/project_manager_pb.js';

type Handlers = Parameters<typeof memoryQueryService<typeof ProjectManager>>[1];

function open(handlers: Handlers | MemoryQueryService) {
  const service = 'call' in handlers ? handlers : memoryQueryService(ProjectManager, handlers);
  const { channel } = createMemoryModuleChannel(schema, { service });
  const host = document.createElement('div');
  document.body.append(host);
  const dispose = render(() => createComponent(page.Page, { client: createModuleClient(channel) }), host);
  const rows = () => [...host.querySelectorAll<HTMLElement>('.pm-list .rows li')];
  const select = (text: string) =>
    rows()
      .find((row) => row.textContent?.includes(text))
      ?.click();
  return { host, dispose, rows, select };
}

const summary = {
  found: true,
  slot: 2,
  name: 'LIVE',
  stamp: 0x6a1f02c3,
  counter: 14,
  modules: [
    { id: 'test/sine', hash: 1, settings: 3, loaded: true },
    { id: 'test/dyn-demo', hash: 2, settings: 1, loaded: false },
  ],
};

describe('the Project Manager page', () => {
  it('lists the projects by number and name and marks the current one and the ones without data', async () => {
    const view = open({
      list: () => ({
        items: [
          { slot: 0, name: 'INTRO', current: false, hasData: true, dataUnknown: false },
          { slot: 2, name: 'LIVE', current: true, hasData: true, dataUnknown: false },
          { slot: 3, name: 'EMPTY', current: false, hasData: false, dataUnknown: false },
          { slot: 4, name: 'SOON', current: false, hasData: false, dataUnknown: true },
        ],
        next: 0,
      }),
      get: () => ({}),
    });
    await vi.waitFor(() => expect(view.rows()).toHaveLength(4));
    const text = view.rows().map((row) => row.textContent);
    expect(text[0]).toContain('001  INTRO');
    expect(text[1]).toContain('003  LIVE');
    expect(text[1]).toContain('current');
    expect(text[2]).toContain('no digikit data');
    expect(text[3]).toContain('…');
    expect(text[3]).not.toContain('no digikit data');
    expect(text[0]).not.toContain('no digikit data');
    view.dispose();
  });

  it('pages: more projects asks for the slot the reply names as next', async () => {
    const asked: number[] = [];
    const slots = Array.from({ length: 20 }, (_, i) => i * 2);
    const view = open({
      list: ({ firstSlot }) => {
        asked.push(firstSlot);
        const items = slots.filter((s) => s >= firstSlot).slice(0, 16);
        const last = items[items.length - 1];
        return {
          items: items.map((slot) => ({ slot, name: `P${slot}`, hasData: true })),
          next: last < slots[slots.length - 1] ? last + 1 : 0,
        };
      },
      get: () => ({}),
    });
    await vi.waitFor(() => expect(view.rows()).toHaveLength(16));
    const more = [...view.host.querySelectorAll('button')].find((b) => b.textContent === 'More projects');
    more?.click();
    await vi.waitFor(() => expect(view.rows()).toHaveLength(20));
    expect(asked).toEqual([0, 31]);
    expect(view.host.textContent).not.toContain('More projects');
    view.dispose();
  });

  it('shows the details of the current project at once', async () => {
    const view = open({
      list: () => ({ items: [{ slot: 2, name: 'LIVE', current: true, hasData: true }], next: 0 }),
      get: () => summary,
    });
    await vi.waitFor(() => expect(view.rows()).toHaveLength(1));
    expect(view.host.textContent).toContain('Select a project');
    view.select('LIVE');
    await vi.waitFor(() => expect(view.host.querySelector('.pm-details dl')).not.toBeNull());
    const details = view.host.querySelector('.pm-details')?.textContent ?? '';
    expect(details).toContain('003');
    expect(details).toContain('LIVE');
    expect(details).toContain('14');
    expect(details).toContain('6A1F02C3');
    expect(details).toContain('test/sine');
    expect(details).toContain('3 settings');
    expect(details).toContain('1 setting');
    expect(details).toContain('not loaded');
    expect(details).not.toContain('Loading');
    view.dispose();
  });

  it('shows Loading… for another project until the device has read it', async () => {
    let calls = 0;
    const view = open({
      list: () => ({ items: [{ slot: 2, name: 'LIVE', hasData: true }], next: 0 }),
      get: () => (++calls < 3 ? { loading: true } : summary),
    });
    await vi.waitFor(() => expect(view.rows()).toHaveLength(1));
    view.select('LIVE');
    await vi.waitFor(() => expect(view.host.textContent).toContain('Loading…'));
    await vi.waitFor(() => expect(view.host.querySelector('.pm-details dl')).not.toBeNull(), { timeout: 2000 });
    expect(calls).toBe(3);
    expect(view.host.textContent).not.toContain('Loading…');
    view.dispose();
  });

  it('says so when the device has no data for the project, and shows a failed query plainly', async () => {
    const view = open({
      list: () => ({
        items: [
          { slot: 5, name: 'GONE', hasData: true },
          { slot: 6, name: 'BAD', hasData: true },
        ],
        next: 0,
      }),
      get: ({ slot }) => {
        if (slot === 6) throw new Error('refused');
        return { found: false };
      },
    });
    await vi.waitFor(() => expect(view.rows()).toHaveLength(2));
    view.select('GONE');
    await vi.waitFor(() => expect(view.host.textContent).toContain('no data for project 006'));
    view.select('BAD');
    await vi.waitFor(() =>
      expect(view.host.querySelector('.pm-details [role=alert]')?.textContent).toContain('refused the request'),
    );
    view.dispose();
  });

  it('stops asking when the selection changes and when the page closes', async () => {
    vi.useFakeTimers();
    onTestFinished(() => void vi.useRealTimers());
    const asked: number[] = [];
    const view = open({
      list: () => ({
        items: [
          { slot: 1, name: 'A', hasData: true },
          { slot: 2, name: 'B', hasData: true },
        ],
        next: 0,
      }),
      get: ({ slot }) => {
        asked.push(slot);
        return { loading: true };
      },
    });
    await vi.waitFor(() => expect(view.rows()).toHaveLength(2));
    view.select('A');
    await vi.waitFor(() => expect(asked.filter((s) => s === 1).length).toBeGreaterThanOrEqual(2), { timeout: 2000 });
    view.select('B');
    await vi.waitFor(() => expect(asked).toContain(2));
    const forA = asked.filter((s) => s === 1).length;
    await vi.advanceTimersByTimeAsync(500);
    expect(asked.filter((s) => s === 1).length).toBeLessThanOrEqual(forA + 1); // at most the reply that was already in flight
    expect(asked.filter((s) => s === 2).length).toBeGreaterThanOrEqual(2);
    view.dispose();
    await vi.advanceTimersByTimeAsync(50);
    const total = asked.length;
    await vi.advanceTimersByTimeAsync(600);
    expect(asked.length).toBeLessThanOrEqual(total + 1);
  });

  describe('saved settings', () => {
    const projectList = { list: () => ({ items: [{ slot: 2, name: 'LIVE', current: true, hasData: true }], next: 0 }) };
    const entry = (key: string, text: string, valueLength = text.length) => ({
      key,
      value: new TextEncoder().encode(text),
      valueLength,
    });
    const settingRows = (view: ReturnType<typeof open>) =>
      [...view.host.querySelectorAll<HTMLElement>('.pm-details .pm-setting')].map((row) => row.textContent ?? '');
    const moduleRow = (view: ReturnType<typeof open>, id: string) =>
      [...view.host.querySelectorAll<HTMLElement>('.pm-details li.pm-module')].find((row) => row.textContent?.includes(id));
    async function selectLive(view: ReturnType<typeof open>) {
      await vi.waitFor(() => expect(view.rows()).toHaveLength(1));
      view.select('LIVE');
      await vi.waitFor(() => expect(view.host.querySelector('.pm-details li.pm-module')).not.toBeNull());
    }

    it('expands a module row to its settings as text, hex and a size for a longer value, and collapses again', async () => {
      const asked: unknown[] = [];
      const view = open({
        ...projectList,
        get: () => summary,
        getSettings: (request) => {
          asked.push(request);
          return {
            found: true,
            entries: [
              entry('shape', 'triangle'),
              { key: 'curve', value: new Uint8Array([0, 0x7f, 0xff]), valueLength: 3 },
              entry('notes', 'x'.repeat(64), 100),
            ],
          };
        },
      });
      await selectLive(view);
      expect(settingRows(view)).toEqual([]);
      moduleRow(view, 'test/sine')?.click();
      await vi.waitFor(() => expect(settingRows(view)).toHaveLength(3));
      expect(asked).toMatchObject([{ slot: 2, module: 'test/sine', version: 14, cursor: 0 }]);
      const [shape, curve, notes] = settingRows(view);
      expect(shape).toContain('shape');
      expect(shape).toContain('triangle');
      expect(shape).not.toContain('bytes');
      expect(curve).toContain('00 7F FF');
      expect(notes).toContain('x'.repeat(64));
      expect(notes).toContain('100 bytes');
      moduleRow(view, 'test/sine')?.click();
      await vi.waitFor(() => expect(settingRows(view)).toEqual([]));
      view.dispose();
    });

    it('does not ask for a module that holds no settings', async () => {
      const getSettings = vi.fn(() => ({ found: true }));
      const view = open({ ...projectList, get: () => ({ ...summary, modules: [{ id: 'a/b', hash: 1, settings: 0, loaded: true }] }), getSettings });
      await selectLive(view);
      moduleRow(view, 'a/b')?.click();
      await vi.waitFor(() => expect(settingRows(view).join()).toContain('No settings saved.'));
      expect(getSettings).not.toHaveBeenCalled();
      view.dispose();
    });

    it('pages with More, asking for the cursor the reply names, and drops More at the end', async () => {
      const asked: number[] = [];
      const view = open({
        ...projectList,
        get: () => summary,
        getSettings: ({ cursor }) => {
          asked.push(cursor);
          return cursor === 0
            ? { found: true, entries: [entry('k1', 'one'), entry('k2', 'two')], next: 2 }
            : { found: true, entries: [entry('k3', 'three')], next: 0 };
        },
      });
      await selectLive(view);
      moduleRow(view, 'test/sine')?.click();
      await vi.waitFor(() => expect(settingRows(view)).toHaveLength(3)); // two entries and More
      const more = () => [...view.host.querySelectorAll('button')].find((b) => b.textContent === 'More');
      more()?.click();
      await vi.waitFor(() => expect(settingRows(view).filter((row) => row.includes('k'))).toHaveLength(3));
      expect(asked).toEqual([0, 2]);
      expect(more()).toBeUndefined();
      view.dispose();
    });

    it('shows Loading… and asks again every 200 ms until the device has read the file, and stops when collapsed', async () => {
      vi.useFakeTimers();
      onTestFinished(() => void vi.useRealTimers());
      let calls = 0;
      const view = open({
        ...projectList,
        get: () => summary,
        getSettings: () => {
          calls++;
          return calls < 3 ? { loading: true } : { found: true, entries: [entry('k', 'v')] };
        },
      });
      await selectLive(view);
      moduleRow(view, 'test/sine')?.click();
      await vi.waitFor(() => expect(view.host.textContent).toContain('Loading…'));
      await vi.advanceTimersByTimeAsync(450);
      await vi.waitFor(() => expect(settingRows(view).join()).toContain('v'));
      expect(calls).toBe(3);
      expect(view.host.querySelector('.pm-details [role=status]')).toBeNull();
      view.dispose();
    });

    it('stops asking when the module is collapsed or another project is selected', async () => {
      vi.useFakeTimers();
      onTestFinished(() => void vi.useRealTimers());
      let calls = 0;
      const view = open({
        list: () => ({
          items: [
            { slot: 2, name: 'LIVE', hasData: true },
            { slot: 3, name: 'OTHER', hasData: true },
          ],
          next: 0,
        }),
        get: ({ slot }) => ({ ...summary, slot }),
        getSettings: () => {
          calls++;
          return { loading: true };
        },
      });
      await vi.waitFor(() => expect(view.rows()).toHaveLength(2));
      view.select('LIVE');
      await vi.waitFor(() => expect(view.host.querySelector('.pm-details li.pm-module')).not.toBeNull());
      moduleRow(view, 'test/sine')?.click();
      await vi.advanceTimersByTimeAsync(450);
      expect(calls).toBeGreaterThanOrEqual(2);
      moduleRow(view, 'test/sine')?.click();
      await vi.advanceTimersByTimeAsync(50);
      const afterCollapse = calls;
      await vi.advanceTimersByTimeAsync(1000);
      expect(calls).toBeLessThanOrEqual(afterCollapse + 1); // at most the reply that was in flight
      moduleRow(view, 'test/sine')?.click();
      await vi.advanceTimersByTimeAsync(250);
      view.select('OTHER');
      await vi.advanceTimersByTimeAsync(100);
      const afterSelect = calls;
      await vi.advanceTimersByTimeAsync(1000);
      expect(calls).toBeLessThanOrEqual(afterSelect + 1);
      view.dispose();
    });

    it('reads the project again when the device says it changed, and asks with the new write counter', async () => {
      let counter = 14;
      const asked: number[] = [];
      const view = open({
        ...projectList,
        get: () => ({ ...summary, counter }),
        getSettings: ({ version }) => {
          asked.push(version);
          if (version !== 15) {
            counter = 15; // the project was saved meanwhile
            return { changed: true };
          }
          return { found: true, entries: [entry('k', 'fresh')] };
        },
      });
      await selectLive(view);
      moduleRow(view, 'test/sine')?.click();
      await vi.waitFor(() => expect(settingRows(view).join()).toContain('fresh'));
      expect(asked).toEqual([14, 15]);
      expect(view.host.querySelector('.pm-details')?.textContent).toContain('15');
      view.dispose();
    });

    it('says so when the module is not in the project and shows a failed query plainly', async () => {
      const view = open({
        ...projectList,
        get: () => summary,
        getSettings: ({ module }) => {
          if (module === 'test/dyn-demo') throw new Error('refused');
          return { found: false };
        },
      });
      await selectLive(view);
      moduleRow(view, 'test/sine')?.click();
      await vi.waitFor(() => expect(view.host.textContent).toContain('holds no settings for this module'));
      moduleRow(view, 'test/dyn-demo')?.click();
      await vi.waitFor(() =>
        expect(view.host.querySelector('.pm-details .pm-setting[role=alert]')?.textContent).toContain('refused the request'),
      );
      view.dispose();
    });
  });
});
