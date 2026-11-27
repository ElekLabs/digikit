// @vitest-environment happy-dom
import { render } from 'solid-js/web';
import { describe, expect, it, vi } from 'vitest';
import {
  createMemoryModuleChannel,
  createModuleClient,
  memoryQueryService,
  type MemoryQueryService,
} from '@digi/module-ui';
import page from '../../../../modules/tool/digikit/project-manager/workbench/index';
import { schema } from '../../../../modules/tool/digikit/project-manager/workbench/preview';
import { ProjectManager } from '../../../../modules/tool/digikit/project-manager/workbench/gen/project_manager_pb.js';

type Handlers = Parameters<typeof memoryQueryService<typeof ProjectManager>>[1];

function open(handlers: Handlers | MemoryQueryService) {
  const service = 'call' in handlers ? handlers : memoryQueryService(ProjectManager, handlers);
  const { channel } = createMemoryModuleChannel(schema, { service });
  const host = document.createElement('div');
  document.body.append(host);
  const dispose = render(() => <page.Page client={createModuleClient(channel)} />, host);
  const rows = () => [...host.querySelectorAll<HTMLButtonElement>('.pm-project')];
  const select = (text: string) =>
    rows()
      .find((row) => row.textContent?.includes(text))
      ?.click();
  return { host, dispose, rows, select };
}

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const summary = {
  found: true,
  slot: 2,
  name: 'LIVE',
  stamp: 0x6a1f02c3,
  counter: 14,
  modules: [
    { id: 'digikit/pluck', hash: 1, settings: 3, loaded: true },
    { id: 'digikit/shot', hash: 2, settings: 1, loaded: false },
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
    expect(details).toContain('digikit/pluck');
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
    await wait(500);
    expect(asked.filter((s) => s === 1).length).toBeLessThanOrEqual(forA + 1); // at most the reply that was already in flight
    expect(asked.filter((s) => s === 2).length).toBeGreaterThanOrEqual(2);
    view.dispose();
    await wait(50);
    const total = asked.length;
    await wait(600);
    expect(asked.length).toBeLessThanOrEqual(total + 1);
  });
});
