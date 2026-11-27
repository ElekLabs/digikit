import { describe, expect, it } from 'vitest';
import { HOST_COUNT, HOST_DIR_MAGIC, HOST_DIR_READY, HOST_ID } from '../src/gen_host_dir.ts';
import { HOST_DIR_BYTES, HostDirectoryError, hostAddress, readHostAddress } from '../src/host-directory.ts';

/** The directory as the device holds it: big-endian words. */
function directory(
  entries: Partial<Record<keyof typeof HOST_ID, number>>,
  count = HOST_COUNT,
  magic = HOST_DIR_MAGIC,
  ready = HOST_DIR_READY,
) {
  const bytes = Buffer.alloc(HOST_DIR_BYTES);
  bytes.writeUInt32BE(magic, 0);
  bytes.writeUInt32BE(count, 4);
  bytes.writeUInt32BE(ready, 8);
  for (const [name, address] of Object.entries(entries))
    bytes.writeUInt32BE(address, 12 + 4 * HOST_ID[name as keyof typeof HOST_ID]);
  return bytes;
}
const reason = (f: () => unknown) => {
  try {
    f();
  } catch (e) {
    return e instanceof HostDirectoryError ? e.reason : String(e);
  }
  return 'none';
};

describe('host directory', () => {
  it('gives the address a package published for its id', () => {
    const dir = directory({ PROVIDERS: 0x4012_3400, TABLE_BAD: 0x4012_9000 });
    expect(hostAddress(dir, 'PROVIDERS')).toBe(0x4012_3400);
    expect(hostAddress(dir, 'TABLE_BAD')).toBe(0x4012_9000);
  });

  it('says no package is loaded for an empty entry, and names the static', () => {
    const dir = directory({});
    expect(reason(() => hostAddress(dir, 'DSP'))).toBe('no-package');
    expect(() => hostAddress(dir, 'DSP')).toThrow(/no package loaded.*DKRT_DSP/);
  });

  it('says no package is loaded while the core has not marked the directory ready, even with entries filled', () => {
    const filling = directory({ PROVIDERS: 0x4012_3400 }, HOST_COUNT, HOST_DIR_MAGIC, 0);
    expect(reason(() => hostAddress(filling, 'PROVIDERS'))).toBe('no-package');
    expect(() => hostAddress(filling, 'PROVIDERS')).toThrow(/still starting/);
  });

  it('tells a core older than this tool, and bytes that are no directory', () => {
    expect(reason(() => hostAddress(directory({ DSP: 4 }, 1), 'PROVIDERS'))).toBe('unknown-id');
    expect(reason(() => hostAddress(directory({}, HOST_COUNT, 0x1234), 'DSP'))).toBe('not-a-directory');
    expect(reason(() => hostAddress(new Uint8Array(4), 'DSP'))).toBe('not-a-directory');
  });

  it('reads the directory through the peek it is given', async () => {
    const dir = directory({ TICKS: 0x4000_2000 });
    const seen: [number, number][] = [];
    const peek = async (addr: number, len: number) => {
      seen.push([addr, len]);
      return dir;
    };
    expect(await readHostAddress(peek, 0x4000_0400, 'TICKS')).toBe(0x4000_2000);
    expect(seen).toEqual([[0x4000_0400, HOST_DIR_BYTES]]);
  });
});
