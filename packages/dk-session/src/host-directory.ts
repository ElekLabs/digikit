// The host directory of the runtime (`DKRT_HOST_DIR`, runtime/core/abi.toml `[[host_dir]]`): how a tool finds a static of the system package.
// The package is a card file placed at a different address each boot, so none of its statics has a symbol in the image. The resident core keeps one table of
// their addresses: a magic, the count of ids that core knows, a ready word and one 32-bit big-endian address per id. Read the one resident symbol, ask for the id, read the
// memory there. An empty entry, or a directory that is not ready (cleared, or the package still starting), means no package is running (a core-only boot, or no `digikit/system.dksp`): the error says so. The Rust reader is
// `dkaddrmap::host_dir`; the ids are generated from abi.toml (`gen_host_dir.ts`).
import { HOST_COUNT, HOST_DIR_MAGIC, HOST_DIR_READY, HOST_ID } from './gen_host_dir.ts';

export { HOST_COUNT, HOST_DIR_MAGIC, HOST_DIR_READY, HOST_ID };

/** The name of a package static in the directory (`DSP` is `DKRT_DSP`). */
export type HostName = keyof typeof HOST_ID;

/** The resident symbol of the directory. */
export const HOST_DIR_SYMBOL = 'DKRT_HOST_DIR';
/** The bytes of the directory a tool reads: the magic, the count, the ready word and one word per id this build knows. */
export const HOST_DIR_BYTES = 4 * (3 + HOST_COUNT);

/** No package is loaded, or the directory does not know the static. */
export class HostDirectoryError extends Error {
  readonly reason: 'not-a-directory' | 'no-package' | 'unknown-id';
  constructor(reason: 'not-a-directory' | 'no-package' | 'unknown-id', message: string) {
    super(message);
    this.name = 'HostDirectoryError';
    this.reason = reason;
  }
}

const word = (b: Uint8Array, at: number): number =>
  ((b[at]! << 24) | (b[at + 1]! << 16) | (b[at + 2]! << 8) | b[at + 3]!) >>> 0;

const NAMES = Object.entries(HOST_ID)
  .sort(([, a], [, b]) => a - b)
  .map(([name]) => name as HostName);

/** The address of package static `which` (its name, or its id as `patchkit addr --format reads` gives it as `host_id`) from the bytes of the directory (a read of `HOST_DIR_SYMBOL`, `HOST_DIR_BYTES` long). */
export function hostAddress(directory: Uint8Array, which: HostName | number): number {
  const name = typeof which === 'number' ? NAMES[which] : which;
  if (name === undefined)
    throw new HostDirectoryError(
      'unknown-id',
      `host directory id ${which} is not one of the ${HOST_COUNT} this build knows`,
    );
  const symbol = `DKRT_${name}`;
  if (directory.length < 12 || word(directory, 0) !== HOST_DIR_MAGIC)
    throw new HostDirectoryError(
      'not-a-directory',
      `${HOST_DIR_SYMBOL} is not a host directory (the image has none, or the read was elsewhere)`,
    );
  const id = HOST_ID[name];
  if (id >= word(directory, 4) || 12 + 4 * id + 4 > directory.length)
    throw new HostDirectoryError(
      'unknown-id',
      `this core does not know ${symbol}: its host directory is older than this tool`,
    );
  // the core marks the directory ready last: a read while the package starts is "no package", not a half-filled table
  const address = word(directory, 12 + 4 * id);
  if (word(directory, 8) !== HOST_DIR_READY || address === 0)
    throw new HostDirectoryError(
      'no-package',
      `no package loaded: the host directory has no address for ${symbol} (the resident core runs alone, the card has no system package, or the package is still starting)`,
    );
  return address;
}

/** Read the directory at `at` (the plan's `DKRT_HOST_DIR`) with `peek` and give the address of package static `name`. */
export async function readHostAddress(
  peek: (addr: number, len: number) => Promise<Uint8Array>,
  at: number,
  name: HostName,
): Promise<number> {
  return hostAddress(await peek(at, HOST_DIR_BYTES), name);
}
