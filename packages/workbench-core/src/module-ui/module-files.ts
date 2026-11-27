import type { DkSession } from '@digi/dk-session';
import {
  listModuleFiles,
  moduleFilePath,
  moduleFilesFolder,
  putModuleFile,
  readStoreFile,
  removeModuleFile,
} from '../modules/dktransfer.ts';

export interface ModuleFileClient {
  list(): Promise<{ name: string; size: number }[]>;
  read(name: string): Promise<Uint8Array>;
  send(name: string, bytes: Uint8Array, progress: (sent: number, total: number) => void): Promise<void>;
  remove(name: string): Promise<void>;
}

export function bindModuleFiles(
  id: string,
  binding: { session(): Promise<DkSession>; check(): void },
  current: () => boolean,
): ModuleFileClient {
  const folder = moduleFilesFolder(id);
  function check() {
    if (!current()) throw new Error('The module page has closed.');
    binding.check();
  }
  async function request<T>(run: (session: DkSession) => Promise<T>) {
    check();
    const session = await binding.session();
    check();
    const result = await run(session);
    check();
    return result;
  }
  return {
    list: () =>
      request(async (s) =>
        (await listModuleFiles(s, id)).map((entry) => ({
          name: entry.path.slice(folder.length),
          size: entry.size,
        })),
      ),
    read: (name) => request((s) => readStoreFile(s, moduleFilePath(id, name))),
    send: (name, bytes, progress) => request((s) => putModuleFile(s, id, name, bytes, progress, check)),
    remove: (name) => request((s) => removeModuleFile(s, id, name, check)),
  };
}
