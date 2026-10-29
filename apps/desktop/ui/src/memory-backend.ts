// What the Memory panel reads: CAPACITY and MODULES of the device behind a session, and the local library (transfer-backend.ts: the built modules
// of the library folder, with their needs from patchkit.wasm). The page speaks the protocol itself over the session of target.ts; no process is
// started. The preview replaces this module (preview/memory-backend.mock.ts).
import { DkError, hasCap, type DkSession } from '@digi/dk-session';
import { buildBars, type Bar, type KnownModule } from './memory-core';
import { loadLibrary } from '@web/transfer/transfer-backend';

export interface MemoryReading {
  /** The device the session talks to: the INFO port name (`dt2-1.17`). */
  port: string;
  bars: Bar[];
  /** How many modules the runtime has loaded. */
  loaded: number;
}

/** Read the device: CAPACITY, MODULES, and the library for what each loaded module takes. A library that cannot be read leaves every module "other". */
export async function readMemory(s: DkSession): Promise<MemoryReading> {
  const info = await s.info();
  if (!hasCap(info, 'CAPACITY'))
    throw new DkError({
      error: 'this runtime does not report its memory (no CAPACITY); update the runtime',
      kind: 'request',
    });
  const caps = await s.capacity();
  const loaded = hasCap(info, 'MODULES') ? (await s.modules()).map((m) => m.name) : [];
  let known: KnownModule[] = [];
  if (loaded.length)
    try {
      known = (await loadLibrary(info.port)).map((m) => ({
        name: m.name,
        label: m.label,
        group: m.group,
        needs: m.needs,
      }));
    } catch {
      // no library folder or no needs reader: the figures stay, the modules are not told apart
    }
  return { port: info.port, bars: buildBars(caps, loaded, known), loaded: loaded.length };
}
