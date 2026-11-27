import type { FsHost } from '@digi/workbench-core/host/host.ts';
import type { LocalEntry } from '@digi/workbench-core/transfer/samples-core.ts';

export type SampleFileHost = Pick<FsHost, 'pickFolder' | 'list' | 'read'>;

export interface SampleSourceFiles {
  pick(start: string): Promise<string | null>;
  list(root: string, path: string, purpose: 'read' | 'poll' | 'walk'): Promise<LocalEntry[] | undefined>;
  read(root: string, path: string, head?: number): Promise<Uint8Array>;
}

export function createSampleSourceFiles(fs: SampleFileHost): SampleSourceFiles {
  return {
    pick: (start) => fs.pickFolder({ start: start || null }),
    list: (root, path) => fs.list(root, path),
    read: async (root, path, head) => new Uint8Array(await fs.read(root, path, head ?? null)),
  };
}
