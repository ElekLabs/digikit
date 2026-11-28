// @vitest-environment happy-dom
import { createComponent } from 'solid-js';
import { render } from 'solid-js/web';
import { describe, expect, it, vi } from 'vitest';
import {
  createMemoryModuleChannel,
  createModuleClient,
  memoryQueryService,
} from '@digi/module-ui';
import page from '../workbench/index';
import { schema } from '../workbench/preview';
import { ProjectManager } from '../workbench/gen/project_manager_pb.js';
import { projectFileName } from '../workbench/project-file';

type Handlers = Parameters<typeof memoryQueryService<typeof ProjectManager>>[1];

/** Two projects whose files are 1000 and 20 bytes: the first needs two pages of 768 bytes. */
const files = new Map<number, Uint8Array>([
  [0, Uint8Array.from({ length: 1000 }, (_, i) => (i === 0 ? 0x44 : i % 251))],
  [2, Uint8Array.from({ length: 20 }, (_, i) => 0x50 + i)],
]);
const names = new Map([[0, 'INTRO'], [2, 'A/B:C']]);

function open(extra: Partial<Handlers> = {}, counter = 5) {
  const asked: { slot: number; version: number; offset: number }[] = [];
  const handlers = {
    list: () => ({
      items: [...files.keys()].map((slot) => ({ slot, name: names.get(slot)!, hasData: true })),
      next: 0,
    }),
    get: ({ slot }) => ({ found: true, slot, name: names.get(slot), counter, modules: [] }),
    readFile: ({ slot, version, offset }) => {
      asked.push({ slot, version, offset });
      const file = files.get(slot)!;
      return { found: true, data: file.slice(offset, offset + 768), total: file.length };
    },
    ...extra,
  } as Handlers;
  const service = memoryQueryService(ProjectManager, handlers);
  const { channel, inspect } = createMemoryModuleChannel(schema, { service });
  const host = document.createElement('div');
  document.body.append(host);
  const dispose = render(() => createComponent(page.Page, { client: createModuleClient(channel) }), host);
  const rows = () => [...host.querySelectorAll<HTMLElement>('.pm-list .rows li')];
  const boxes = () => [...host.querySelectorAll<HTMLInputElement>('.pm-list input[type=checkbox]')];
  const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent?.trim() === text) as HTMLButtonElement | undefined;
  return { host, dispose, rows, boxes, button, asked, inspect };
}

describe('the Raw tab', () => {
  it('fetches every page of the file and shows it as hex with byte offsets, coloured by section', async () => {
    const view = open();
    await vi.waitFor(() => expect(view.rows()).toHaveLength(2));
    view.rows()[1].click();
    await vi.waitFor(() => expect(view.button('Raw')).toBeDefined());
    view.button('Raw')!.click();
    await vi.waitFor(() => expect(view.host.querySelectorAll('.pm-hex-line')).toHaveLength(2));
    expect(view.asked).toEqual([{ slot: 2, version: 5, offset: 0 }]);
    const lines = [...view.host.querySelectorAll('.pm-hex-line')];
    expect(lines[0].querySelector('.pm-hex-offset')?.textContent).toBe('0000');
    expect(lines[1].querySelector('.pm-hex-offset')?.textContent).toBe('0010');
    expect(lines[1].querySelectorAll('.pm-byte')).toHaveLength(4);
    // 0x50 0x51 0x52 0x53 is no known version: the bytes are plain
    expect(view.host.querySelector('.pm-byte.pm-k-magic')).toBeNull();
    view.dispose();
  });

  it('shows an ASCII column of 16 characters per row beside the hex cells', async () => {
    const view = open();
    await vi.waitFor(() => expect(view.rows()).toHaveLength(2));
    view.rows()[0].click();
    await vi.waitFor(() => expect(view.button('Raw')).toBeDefined());
    view.button('Raw')!.click();
    await vi.waitFor(() => expect(view.host.querySelectorAll('.pm-hex-line')).toHaveLength(63));
    const first = view.host.querySelector('.pm-hex-line')!;
    expect(first.querySelector('.pm-ascii')?.textContent).toHaveLength(16);
    // byte 0 is 0x44 'D', byte 1 is 0x01 (not printable)
    expect(first.querySelector('.pm-ascii')?.textContent?.slice(0, 2)).toBe('D.');
    view.dispose();
  });

  it('asks for the next page at the offset it has and joins them', async () => {
    const view = open();
    await vi.waitFor(() => expect(view.rows()).toHaveLength(2));
    view.rows()[0].click();
    await vi.waitFor(() => expect(view.button('Raw')).toBeDefined());
    view.button('Raw')!.click();
    await vi.waitFor(() => expect(view.host.querySelectorAll('.pm-hex-line')).toHaveLength(63));
    expect(view.asked.map((a) => a.offset)).toEqual([0, 768]);
    expect(view.host.querySelectorAll('.pm-byte')).toHaveLength(1000);
    view.dispose();
  });

  it('colours a file of a known version and shows its header fields', async () => {
    const dkp = Uint8Array.from([
      ...new TextEncoder().encode('DKPJ'), 0, 1, 0x12, 0x34, 3, 2, 0x41, 0x42, ...Array.from({ length: 14 }, () => 0),
      0, 0, 0, 9, 0, 0, 0, 7, 0, 0, 0, 42, 1, 2, 3, 4,
    ]);
    const view = open({ readFile: () => ({ found: true, data: dkp, total: dkp.length }) });
    await vi.waitFor(() => expect(view.rows()).toHaveLength(2));
    view.rows()[1].click();
    await vi.waitFor(() => expect(view.button('Raw')).toBeDefined());
    view.button('Raw')!.click();
    await vi.waitFor(() => expect(view.host.querySelector('.pm-byte.pm-k-magic')).not.toBeNull());
    const kinds = (kind: string) => view.host.querySelectorAll(`.pm-byte.pm-k-${kind}`).length;
    // every full row has 16 byte cells of one structure: pm-byte, plus at most a pm-k-* section colour
    const lines = [...view.host.querySelectorAll('.pm-hex-line')];
    for (const [index, line] of lines.entries()) {
      const cells = [...line.querySelectorAll('.pm-byte')];
      if (index < lines.length - 1) expect(cells).toHaveLength(16);
      for (const cell of cells) expect([...cell.classList].filter((c) => !c.startsWith('pm-k-'))).toEqual(['pm-byte']);
      for (const cell of cells) expect(cell.classList.length).toBeLessThanOrEqual(2);
    }
    expect([kinds('magic'), kinds('version'), kinds('device'), kinds('slot'), kinds('counter'), kinds('used'), kinds('crc')]).toEqual([4, 2, 2, 1, 4, 4, 4]);
    view.dispose();
  });

  it('lists the decoded fields with the checksum state and marks the bytes of the hovered field in hex and ASCII', async () => {
    const dkp = Uint8Array.from([
      ...new TextEncoder().encode('DKPJ'), 0, 1, 0x12, 0x34, 3, 2, 0x41, 0x42, ...Array.from({ length: 14 }, () => 0),
      0, 0, 0, 9, 0, 0, 0, 7, 0, 0, 0, 42, 1, 2, 3, 4,
    ]);
    const view = open({ readFile: () => ({ found: true, data: dkp, total: dkp.length }) });
    await vi.waitFor(() => expect(view.rows()).toHaveLength(2));
    view.rows()[1].click();
    await vi.waitFor(() => expect(view.button('Raw')).toBeDefined());
    view.button('Raw')!.click();
    await vi.waitFor(() => expect(view.host.querySelector('.pm-decoded')).not.toBeNull());
    expect(view.host.querySelector('.pm-checksum')?.textContent).toBe('checksum mismatch');
    const row = [...view.host.querySelectorAll<HTMLElement>('.pm-field')].find((f) => f.textContent?.startsWith('device'))!;
    expect(row.querySelector('.pm-field-value')?.textContent).toBe('4660');
    row.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
    row.dispatchEvent(new MouseEvent('mouseenter'));
    const marked = (selector: string) => [...view.host.querySelectorAll<HTMLElement>(`${selector}.pm-hot`)].map((c) => c.dataset.offset);
    await vi.waitFor(() => expect(marked('.pm-byte')).toEqual(['6', '7']));
    expect(marked('.pm-ascii-cell')).toEqual(['6', '7']);
    // hovering a byte marks its decoded row
    view.host.querySelector<HTMLElement>('.pm-byte[data-offset="30"]')!.dispatchEvent(new MouseEvent('mouseenter'));
    await vi.waitFor(() => expect([...view.host.querySelectorAll('.pm-field.pm-hot .pm-field-label')].map((l) => l.textContent)).toEqual(['write counter']));
    view.dispose();
  });

  it('shows no decoded panel for a file of no known version', async () => {
    const view = open();
    await vi.waitFor(() => expect(view.rows()).toHaveLength(2));
    view.rows()[1].click();
    await vi.waitFor(() => expect(view.button('Raw')).toBeDefined());
    view.button('Raw')!.click();
    await vi.waitFor(() => expect(view.host.querySelectorAll('.pm-hex-line')).toHaveLength(2));
    expect(view.host.querySelector('.pm-decoded')).toBeNull();
    view.dispose();
  });

  it('shows Loading… and asks again while the device reads the file, and a failure in words', async () => {
    let calls = 0;
    const view = open({ readFile: () => (++calls < 3 ? { loading: true } : { found: true, data: new Uint8Array([1, 2]), total: 2 }) });
    await vi.waitFor(() => expect(view.rows()).toHaveLength(2));
    view.rows()[1].click();
    await vi.waitFor(() => expect(view.button('Raw')).toBeDefined());
    view.button('Raw')!.click();
    await vi.waitFor(() => expect(view.host.querySelector('.pm-raw')?.textContent).toContain('Loading'));
    await vi.waitFor(() => expect(view.host.querySelectorAll('.pm-hex-line')).toHaveLength(1));
    expect(calls).toBe(3);
    view.dispose();
  });
});

describe('selecting projects and saving them', () => {
  it('disables Save selected until a project is ticked, and ticking does not select the row', async () => {
    const view = open();
    await vi.waitFor(() => expect(view.boxes()).toHaveLength(2));
    expect(view.button('Save selected')!.disabled).toBe(true);
    view.boxes()[1].click();
    await vi.waitFor(() => expect(view.button('Save selected')!.disabled).toBe(false));
    expect(view.host.querySelector('.pm-details')).toBeNull();
    view.boxes()[1].click();
    await vi.waitFor(() => expect(view.button('Save selected')!.disabled).toBe(true));
    view.dispose();
  });

  it('saves the ticked projects as NNN-NAME.dkp with the bytes of their files in one call', async () => {
    const view = open();
    await vi.waitFor(() => expect(view.boxes()).toHaveLength(2));
    view.boxes()[0].click();
    view.boxes()[1].click();
    view.button('Save selected')!.click();
    await vi.waitFor(() => expect(view.inspect.saved).toHaveLength(1));
    const saved = view.inspect.saved[0];
    expect(saved.map((f) => f.name)).toEqual(['001-INTRO.dkp', '003-A_B_C.dkp']);
    expect(saved[0].bytes).toEqual(files.get(0));
    expect(saved[1].bytes).toEqual(files.get(2));
    await vi.waitFor(() => expect(view.host.querySelector('.pm-save-message')?.textContent).toContain('Saved 2 files'));
    view.dispose();
  });

  it('names a project it could not read and still saves the others', async () => {
    const view = open({
      readFile: ({ slot, offset }) =>
        slot === 0 ? { found: false } : { found: true, data: files.get(slot)!.slice(offset), total: files.get(slot)!.length },
    });
    await vi.waitFor(() => expect(view.boxes()).toHaveLength(2));
    view.boxes()[0].click();
    view.boxes()[1].click();
    view.button('Save selected')!.click();
    await vi.waitFor(() => expect(view.inspect.saved).toHaveLength(1));
    expect(view.inspect.saved[0].map((f) => f.name)).toEqual(['003-A_B_C.dkp']);
    await vi.waitFor(() => expect(view.host.querySelector('.pm-save-message')?.textContent).toContain('Could not read project 001'));
    view.dispose();
  });
});

describe('the saved file name', () => {
  it('is the number and the name, with characters a file name cannot hold replaced', () => {
    expect(projectFileName(0, 'INTRO')).toBe('001-INTRO.dkp');
    expect(projectFileName(126, 'A/B:C*?')).toBe('127-A_B_C__.dkp');
    expect(projectFileName(4, '')).toBe('005.dkp');
    expect(projectFileName(4, 'END. ')).toBe('005-END.dkp');
  });
});
