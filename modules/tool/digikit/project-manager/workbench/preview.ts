// The preview's answers to the Project Manager queries: a few fake projects with saved settings (a binary value, a long text, a short text). The module has no
// settings of its own, so this file also gives the preview the (empty) schema it needs to load the module and open its page.
import { memoryQueryService, type MemoryQueryService } from '@digi/module-ui';
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
