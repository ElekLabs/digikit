// The preview's answers to the Project Manager queries: a few fake projects. The module has no settings, so this file also gives the preview the
// (empty) schema it needs to load the module and open its page.
import { memoryQueryService, type MemoryQueryService } from '@digi/module-ui';
import { ProjectManager } from './gen/project_manager_pb.js';

export const schema = { module: 'digikit/project-manager', name: 'Project manager', settings: [] } as const;

const PAGE = 16;

interface FakeProject {
  slot: number;
  name: string;
  hasData: boolean;
  dataUnknown?: boolean;
  stamp?: number;
  counter?: number;
  modules?: { id: string; hash: number; settings: number; loaded: boolean }[];
}

const CURRENT = 1;

const projects: FakeProject[] = [
  { slot: 0, name: 'INTRO', hasData: true, stamp: 0x6a1f02c3, counter: 6, modules: [{ id: 'digikit/shot', hash: 0x1c2d3e4f, settings: 2, loaded: true }] },
  {
    slot: 1,
    name: 'DUB SKETCH',
    hasData: true,
    stamp: 0x6a1f9b10,
    counter: 14,
    modules: [
      { id: 'digikit/pluck', hash: 0x0badf00d, settings: 3, loaded: true },
      { id: 'digikit/shot', hash: 0x1c2d3e4f, settings: 0, loaded: false },
    ],
  },
  { slot: 4, name: 'LIVE SET 2', hasData: false },
  { slot: 9, name: 'NEW', hasData: false, dataUnknown: true },
];

export default function previewQueries(): MemoryQueryService {
  // Like the device, another project than the current one is read from the card first: INTRO answers `loading` twice before its data.
  const waiting = new Map<number, number>([[0, 2]]);
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
        modules: project.modules ?? [],
      };
    },
  });
}
