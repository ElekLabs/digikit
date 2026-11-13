import type { Session } from '@digi/test-support';
import { word } from './fx-live.ts';
import { patchkit } from './patchkit.ts';

/** The runtime's module table (modules/core/runtime/src/rs/src/lib.rs, `State`): ten header words, then one record of 13 words per slot:
 * used, name[32], version, base, size, fini (the same layout `tools/devtools/addrnames.py` reads). */
const STATE_HEADER_BYTES = 40;
const MODULE_RECORD_BYTES = 52;

export interface ModuleLoad {
  id: string;
  base: number;
  size: number;
  /** The address of the module's `fini` function (0: none). */
  fini: number;
}

/** The load table of the running emulator, read from the module table at DKRT_STATE. */
export async function readLoadTable(session: Session, state: number, slots: number): Promise<ModuleLoad[]> {
  const loads: ModuleLoad[] = [];
  for (let slot = 0; slot < slots; slot++) {
    const at = state + STATE_HEADER_BYTES + slot * MODULE_RECORD_BYTES;
    if ((await word(session, at)) === 0) continue;
    const name = Buffer.alloc(32);
    for (let i = 0; i < 8; i++) name.writeUInt32BE(await word(session, at + 4 + 4 * i), 4 * i);
    loads.push({
      id: name.toString('latin1').split('\0')[0]!,
      base: await word(session, at + 40),
      size: await word(session, at + 44),
      fini: await word(session, at + 48),
    });
  }
  return loads;
}

export interface Resolved {
  cpu: string;
  addr: number;
  name: string | null;
  text: string | null;
  layer: 'our' | 'anchor' | 'fw' | null;
  sym_addr: number | null;
  offset: number | null;
  module: string | null;
}

/** `patchkit addr`: the names of ColdFire addresses from address map files and a load table. */
export async function resolveAddresses(
  maps: string[],
  loads: Pick<ModuleLoad, 'id' | 'base'>[],
  addrs: number[],
): Promise<Resolved[]> {
  const { stdout } = await patchkit([
    'addr',
    ...maps.flatMap((m) => ['--map', m]),
    ...loads.flatMap((l) => ['--load', `${l.id}=${l.base.toString(16)}`]),
    ...addrs.map((a) => `0x${a.toString(16)}`),
  ]);
  return JSON.parse(stdout).results as Resolved[];
}
