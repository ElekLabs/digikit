import type { Session } from '@digi/test-support';
import { word } from './fx-live.ts';
import { patchkit } from './patchkit.ts';

/** The runtime's module table (modules/core/runtime/src/rs/src/lib.rs, `State`): ten header words, then one record of 13 words per slot:
 * used, name[32], version, base, size, fini (the same layout `tools/devtools/addrnames.py` reads). */
const STATE_HEADER_BYTES = 40;
const MODULE_RECORD_BYTES = 52;
/** DKRT_DSP (`DspState`, modules/core/runtime/src/rs/src/dsp.rs): 13 words, then mod_off[slots] and mod_len[slots]. The area is word 3 of
 * `dyndsp_info`, an L2 byte address; L2_BYTE_BASE and L2_SW_BASE are the constants of packages/patchkit/src/dsp.rs. */
const DSP_HEADER_BYTES = 52;
const INFO_AREA_BYTES = 12;
const L2_BYTE_BASE = 0x2000_0000;
const L2_SW_BASE = 0x00b8_0000;

export interface ModuleLoad {
  id: string;
  base: number;
  size: number;
  /** The address of the module's `fini` function (0: none). */
  fini: number;
  /** The software PC where the module's DSP image starts; undefined without a DSP part. */
  dspBase?: number;
}

/** The load table of the running emulator, read from the module table at DKRT_STATE. With `dsp` (the symbols DKRT_DSP and dyndsp_info) a module
 * with a DSP part also gets its `dspBase` (the slot index is the DKRT_DSP record index). */
export async function readLoadTable(
  session: Session,
  state: number,
  slots: number,
  dsp?: { state: number; info: number },
): Promise<ModuleLoad[]> {
  const loads: ModuleLoad[] = [];
  const area = dsp ? await word(session, dsp.info + INFO_AREA_BYTES) : 0;
  for (let slot = 0; slot < slots; slot++) {
    const at = state + STATE_HEADER_BYTES + slot * MODULE_RECORD_BYTES;
    if ((await word(session, at)) === 0) continue;
    const name = Buffer.alloc(32);
    for (let i = 0; i < 8; i++) name.writeUInt32BE(await word(session, at + 4 + 4 * i), 4 * i);
    const load: ModuleLoad = {
      id: name.toString('latin1').split('\0')[0]!,
      base: await word(session, at + 40),
      size: await word(session, at + 44),
      fini: await word(session, at + 48),
    };
    if (dsp && area && (await word(session, dsp.state + DSP_HEADER_BYTES + 4 * (slots + slot))) !== 0) {
      const off = await word(session, dsp.state + DSP_HEADER_BYTES + 4 * slot);
      load.dspBase = L2_SW_BASE + (area + off - L2_BYTE_BASE) / 2;
    }
    loads.push(load);
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
  loads: Pick<ModuleLoad, 'id' | 'base' | 'dspBase'>[],
  addrs: number[],
): Promise<Resolved[]> {
  const { stdout } = await patchkit([
    'addr',
    ...maps.flatMap((m) => ['--map', m]),
    ...loads.flatMap((l) => [
      '--load',
      `${l.id}=${l.base.toString(16)}`,
      ...(l.dspBase === undefined ? [] : ['--dsp-base', `${l.id}=${l.dspBase.toString(16)}`]),
    ]),
    ...addrs.map((a) => `0x${a.toString(16)}`),
  ]);
  return JSON.parse(stdout).results as Resolved[];
}
