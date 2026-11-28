// The preview's answers to the Project Manager queries: a few fake projects with saved settings (a binary value, a long text, a short text). The module has no
// settings of its own, so this file also gives the preview the (empty) schema it needs to load the module and open its page.
import { memoryQueryService, type MemoryQueryService } from '@digi/module-ui';
import { crc32 } from './dkp-spans';
import { ProjectManager } from './gen/project_manager_pb.js';

export const schema = { module: 'digikit/project-manager', name: 'Project manager', settings: [] } as const;

const PAGE = 16;
/** The settings of one reply. */
const SETTINGS_PAGE = 4;
const VALUE_SHOWN = 64;

interface FakeProject {
  slot: number;
  name: string;
  hasData: boolean;
  dataUnknown?: boolean;
  stamp?: number;
  counter?: number;
  modules?: { id: string; hash: number; loaded: boolean }[];
  /** The saved settings by module id. */
  saved?: Record<string, { key: string; value: Uint8Array }[]>;
}

const text = (value: string) => new TextEncoder().encode(value);

const CURRENT = 1;

const projects: FakeProject[] = [
  { slot: 0, name: 'INTRO', hasData: true, stamp: 0x6a1f02c3, counter: 6, modules: [{ id: 'test/dyn-demo', hash: 0x1c2d3e4f, loaded: true }],
    saved: {
      'test/dyn-demo': [
        { key: 'curve', value: new Uint8Array([0x00, 0x7f, 0x80, 0xff, 0x10, 0x02]) },
        { key: 'notes', value: text('A long note that the module saved with this project, longer than the sixty-four bytes the device shows of a value in this list.') },
      ],
    },
  },
  {
    slot: 1,
    name: 'DUB SKETCH',
    hasData: true,
    stamp: 0x6a1f9b10,
    counter: 14,
    modules: [
      { id: 'test/sine', hash: 0x0badf00d, loaded: true },
      { id: 'test/dyn-demo', hash: 0x1c2d3e4f, loaded: false },
    ],
    saved: {
      'test/sine': [
        { key: 'shape', value: text('triangle') },
        { key: 'gain', value: text('0.8') },
        { key: 'phase', value: text('0') },
        { key: 'label', value: text('Lead') },
        { key: 'seed', value: new Uint8Array([1, 2, 3, 4]) },
      ],
    },
  },
  { slot: 4, name: 'LIVE SET 2', hasData: false },
  { slot: 9, name: 'NEW', hasData: false, dataUnknown: true },
];

const RAW_PAGE = 768;

const be = (value: number, bytes: number) => Array.from({ length: bytes }, (_, i) => (value >>> (8 * (bytes - 1 - i))) & 0xff);

/** A project file as the device would hold it (docs/design/digikit-project.md section 4): header, the module list and each module's saved settings. The crc is not computed (the preview colours bytes, it does not check them). */
function dkpOf(p: FakeProject): Uint8Array {
  const modules = (p.modules ?? []).flatMap((m) => [m.id.length, ...text(m.id), ...be(m.hash, 4)]);
  const body: number[] = modules.length ? [1, ...be(modules.length, 2), ...modules] : [];
  for (const [id, entries] of Object.entries(p.saved ?? {})) {
    const section = [id.length, ...text(id), ...entries.flatMap((e) => {
      const key = text(`${id}/${e.key}`);
      return [key.length, ...key, ...be(e.value.length, 2), ...e.value];
    })];
    body.push(2, ...be(section.length, 2), ...section);
  }
  const name = text(p.name).slice(0, 16);
  const head = [
    ...text('DKPJ'), ...be(1, 2), ...be(0x1234, 2), p.slot, name.length,
    ...name, ...Array.from({ length: 16 - name.length }, () => 0),
    ...be(p.stamp ?? 0, 4), ...be(p.counter ?? 0, 4), ...be(42 + body.length, 4), ...be(0, 4),
  ];
  const file = Uint8Array.from([...head, ...body]);
  // the stored CRC-32, over the file without its own four bytes
  new DataView(file.buffer).setUint32(38, crc32(file, [[0, 38], [42, file.length]]));
  return file;
}

export default function previewQueries(): MemoryQueryService {
  // Like the device, another project than the current one is read from the card first: INTRO answers `loading` twice before its data.
  const waiting = new Map<number, number>([[0, 2]]);
  // and each page of another project's settings once
  const settingsWaiting = new Set<string>();
  return memoryQueryService(ProjectManager, {
    list: ({ firstSlot }) => {
      const rest = projects.filter((p) => p.slot >= firstSlot);
      const page = rest.slice(0, PAGE);
      return {
        items: page.map((p) => ({
          slot: p.slot,
          name: p.name,
          current: p.slot === CURRENT,
          hasData: p.hasData,
          dataUnknown: p.dataUnknown ?? false,
        })),
        next: rest.length > PAGE ? page[page.length - 1].slot + 1 : 0,
      };
    },
    get: ({ slot }) => {
      const project = projects.find((p) => p.slot === slot);
      if (!project || !project.hasData) return { found: false };
      const left = slot === CURRENT ? 0 : (waiting.get(slot) ?? 0);
      if (left > 0) {
        waiting.set(slot, left - 1);
        return { loading: true };
      }
      return {
        found: true,
        slot,
        name: project.name,
        stamp: project.stamp ?? 0,
        counter: project.counter ?? 0,
        modules: (project.modules ?? []).map((m) => ({ ...m, settings: project.saved?.[m.id]?.length ?? 0 })),
      };
    },
    readFile: ({ slot, version, offset }) => {
      const project = projects.find((p) => p.slot === slot);
      if (!project || !project.hasData) return { found: false };
      if (slot !== CURRENT) {
        const request = `raw/${slot}/${offset}`;
        if (!settingsWaiting.has(request)) {
          settingsWaiting.add(request);
          return { loading: true };
        }
        settingsWaiting.delete(request);
      }
      if ((project.counter ?? 0) !== version) return { changed: true };
      const file = dkpOf(project);
      return { found: true, data: file.slice(offset, offset + RAW_PAGE), total: file.length };
    },
    getSettings: ({ slot, module, version, cursor }) => {
      const project = projects.find((p) => p.slot === slot);
      const saved = project?.saved?.[module];
      if (!project || !project.hasData || !project.modules?.some((m) => m.id === module)) return { found: false };
      if (slot !== CURRENT) {
        const request = `${slot}/${module}/${cursor}`;
        if (!settingsWaiting.has(request)) {
          settingsWaiting.add(request);
          return { loading: true };
        }
        settingsWaiting.delete(request);
      }
      if ((project.counter ?? 0) !== version) return { changed: true };
      const rest = (saved ?? []).slice(cursor);
      const page = rest.slice(0, SETTINGS_PAGE);
      return {
        found: true,
        entries: page.map((e) => ({ key: e.key, value: e.value.slice(0, VALUE_SHOWN), valueLength: e.value.length })),
        next: rest.length > SETTINGS_PAGE ? cursor + SETTINGS_PAGE : 0,
      };
    },
  });
}
