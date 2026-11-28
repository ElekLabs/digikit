import type { ServiceClient } from '@digi/module-ui';
import type { ProjectManager } from './gen/project_manager_pb.js';
import { queryErrorText } from './project-text';

/** How long to wait before asking again for a project the device is still reading. */
export const FILE_RETRY_MS = 200;
/** How often a project that changed while it was read is read again before the read gives up. */
const ATTEMPTS = 3;

export type Pm = ServiceClient<typeof ProjectManager>;

/** The whole `.dkp` file of one project as the device holds it, with the name and write counter its summary had. */
export interface ProjectFile {
  slot: number;
  name: string;
  counter: number;
  bytes: Uint8Array;
}

export type FileRead = { state: 'ok'; file: ProjectFile } | { state: 'missing' } | { state: 'error'; text: string };

/** Reads project `slot`: its summary (name and write counter), then the file page by page. `loading` waits FILE_RETRY_MS and asks again, so does the device's read of
 * another project's file; a project that changed meanwhile (`changed`) is read again from its summary, ATTEMPTS times in all. `active` says whether anyone still wants the
 * result: the read stops quietly (`undefined`) when it turns false. `wait` is the pause between asks. */
export async function readProjectFile(
  service: Pm,
  slot: number,
  active: () => boolean,
  wait: (ms: number) => Promise<void> = (ms) => new Promise((done) => setTimeout(done, ms)),
): Promise<FileRead | undefined> {
  for (let attempt = 0; attempt < ATTEMPTS; attempt++) {
    let name = '';
    let counter = 0;
    for (;;) {
      const result = await service.get({ slot });
      if (!active()) return undefined;
      if (!result.ok) return { state: 'error', text: queryErrorText(result.error) };
      if (result.value.loading) {
        await wait(FILE_RETRY_MS);
        if (!active()) return undefined;
        continue;
      }
      if (!result.value.found) return { state: 'missing' };
      ({ name, counter } = result.value);
      break;
    }
    const chunks: Uint8Array[] = [];
    let offset = 0;
    let total = Infinity;
    let changed = false;
    while (offset < total) {
      const result = await service.readFile({ slot, version: counter, offset });
      if (!active()) return undefined;
      if (!result.ok) return { state: 'error', text: queryErrorText(result.error) };
      const reply = result.value;
      if (reply.loading) {
        await wait(FILE_RETRY_MS);
        if (!active()) return undefined;
        continue;
      }
      if (reply.changed) {
        changed = true;
        break;
      }
      if (!reply.found) return { state: 'missing' };
      total = reply.total;
      // an empty range before the end would never advance: stop instead of asking forever
      if (reply.data.length === 0) break;
      chunks.push(reply.data);
      offset += reply.data.length;
    }
    if (changed) continue;
    const bytes = new Uint8Array(offset);
    let at = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, at);
      at += chunk.length;
    }
    return { state: 'ok', file: { slot, name, counter, bytes } };
  }
  return { state: 'error', text: 'The project keeps changing. Try again.' };
}

/** The file name a saved project gets: its number and name, `NNN-NAME.dkp`. A character a file name cannot hold becomes `_`; the number keeps names apart. */
export function projectFileName(slot: number, name: string): string {
  // eslint-disable-next-line no-control-regex
  const plain = name.replace(/[/\\:*?"<>|\u0000-\u001f\u007f]/g, '_').replace(/[. ]+$/, '');
  const number = String(slot + 1).padStart(3, '0');
  return plain ? `${number}-${plain}.dkp` : `${number}.dkp`;
}
