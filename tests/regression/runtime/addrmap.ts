import type { Session } from '@digi/test-support';
import { word } from './fx-live.ts';
import { patchkit } from './patchkit.ts';
import { planSymbol, type RuntimePlan } from './runtime-plan.ts';

export interface ModuleLoad {
  id: string;
  base: number;
  size: number;
  /** The address of the module's `fini` function (0: none). */
  fini: number;
  /** The software PC where the module's DSP image starts; undefined without a DSP part. */
  dspBase?: number;
}

/**
 * The load table of the running emulator. `patchkit addr --format reads` names the symbols to read (the ship map knows the slot count and the
 * layout); the raw bytes go back to patchkit, which decodes the records (`dkaddrmap::snapshot`).
 */
export async function readLoadTable(session: Session, plan: RuntimePlan, shipMap: string): Promise<ModuleLoad[]> {
  const reads = JSON.parse((await patchkit(['addr', '--map', shipMap, '--format', 'reads'])).stdout) as {
    symbol: string;
    len: number;
  }[];
  const snapshot: string[] = [];
  for (const { symbol, len } of reads) {
    const at = planSymbol(plan, symbol);
    const bytes = Buffer.alloc(Math.ceil(len / 4) * 4);
    for (let i = 0; i < bytes.length; i += 4) bytes.writeUInt32BE(await word(session, at + i), i);
    snapshot.push('--snapshot', `${symbol}=${bytes.subarray(0, len).toString('hex')}`);
  }
  const { stdout } = await patchkit(['addr', '--map', shipMap, ...snapshot, '--format', 'loads']);
  const table = JSON.parse(stdout) as {
    modules: { id: string; base: number; size: number; fini?: number; dsp_base?: number }[];
  };
  return table.modules.map((m) => ({ id: m.id, base: m.base, size: m.size, fini: m.fini ?? 0, dspBase: m.dsp_base }));
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
