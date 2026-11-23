import { describe, expect, it } from 'vitest';
import { HOST_COUNT, HOST_DIR_MAGIC, HOST_DIR_READY, HOST_ID } from '../src/gen_host_dir.ts';
import { HOST_DIR_BYTES } from '../src/host-directory.ts';
import { readLoadTable, type NameProf } from '../src/log-names.ts';

/** A dkprof that asks for what a ship map with the DSP loader and providers asks for (`Loader::reads`), and answers a table whose modules are the snapshot's symbols. */
const prof = {
  loadTable: (req: { snapshot?: Record<string, string> }) =>
    req.snapshot
      ? { modules: Object.keys(req.snapshot).map((id) => ({ id, base: 0 })) }
      : {
          reads: [
            { symbol: 'DKRT_STATE', len: 8 },
            { symbol: 'DKRT_HOST_DIR', len: HOST_DIR_BYTES },
            { symbol: 'DKRT_DSP', len: 4, host_id: HOST_ID.DSP },
            { symbol: 'DKRT_PROVIDERS', len: 4, host_id: HOST_ID.PROVIDERS },
          ],
        },
} as unknown as NameProf;

const DIR_AT = 0x4000_0400;
function memory(published: Partial<Record<keyof typeof HOST_ID, number>>) {
  const dir = Buffer.alloc(HOST_DIR_BYTES);
  dir.writeUInt32BE(HOST_DIR_MAGIC, 0);
  dir.writeUInt32BE(HOST_COUNT, 4);
  dir.writeUInt32BE(HOST_DIR_READY, 8);
  for (const [name, at] of Object.entries(published))
    dir.writeUInt32BE(at, 12 + 4 * HOST_ID[name as keyof typeof HOST_ID]);
  const peeks: number[] = [];
  const peek = async (addr: number, len: number) => {
    peeks.push(addr);
    return addr === DIR_AT ? new Uint8Array(dir) : new Uint8Array(len);
  };
  return { peek, peeks };
}

describe('the load table of an image with a system package', () => {
  it('reads a package static at the address its id has in the host directory', async () => {
    const { peek, peeks } = memory({ DSP: 0x4001_1000, PROVIDERS: 0x4001_2000 });
    const table = await readLoadTable(prof, {}, { DKRT_STATE: 0x4000_0000, DKRT_HOST_DIR: DIR_AT }, peek);
    expect(peeks).toEqual([0x4000_0000, DIR_AT, 0x4001_1000, 0x4001_2000]);
    expect(table.modules.map((m) => m.id)).toContain('DKRT_DSP');
  });

  it('says no package is loaded when the directory is empty, and does not read garbage', async () => {
    const { peek, peeks } = memory({});
    await expect(readLoadTable(prof, {}, { DKRT_STATE: 0x4000_0000, DKRT_HOST_DIR: DIR_AT }, peek)).rejects.toThrow(
      /no package loaded.*DKRT_DSP/,
    );
    expect(peeks).toEqual([0x4000_0000, DIR_AT]);
  });
});
