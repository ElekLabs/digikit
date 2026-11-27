// `StoreOps` over a `DkSession`: the same code reads the store of the emulator and of a device.
import { READ_MAX, storeFree, type DkSession } from '@digi/dk-session';
import { systemPackageReads } from '../runtime/system-package';
import type { TargetApp } from '../transfer/target';
import { factsOf, type RunOps, type StoreOps, type TargetOps } from './ops';
import { parseDkmHeader, usageOf } from './store';

export function sessionStoreOps(session: () => Promise<DkSession>, check?: () => void): StoreOps {
  async function read<T>(call: () => Promise<T>): Promise<T> {
    check?.();
    const answer = await call();
    check?.();
    return answer;
  }
  return {
    async snapshot() {
      const s = await session();
      // one request at a time: SysEx replies come on one stream
      const info = await read(() => s.info());
      const entries = await read(() => s.list());
      const pkg = systemPackageReads(info);
      const cap = await read(() => pkg.command('CAPACITY', () => s.capacity()));
      const modules = await read(() => pkg.command('MODULES', () => s.modules()));
      const states = await read(() => pkg.command('LOADS', () => s.loadStates()));
      return {
        facts: factsOf(info),
        entries,
        usage: usageOf(entries, storeFree(info, pkg.coreOnly ? null : (cap ?? null))),
        modules: pkg.coreOnly ? [] : (modules ?? []),
        states: states ?? [],
        ...(pkg.coreOnly ? { coreOnly: true } : {}),
      };
    },
    async header(path) {
      const s = await session();
      return parseDkmHeader(await read(() => s.header(path)));
    },
    async read(path, offset, length) {
      const s = await session();
      return read(() => s.read(path, offset, length));
    },
    async readAll(path, onProgress) {
      const s = await session();
      const first = await read(() => s.read(path, 0, READ_MAX));
      const size = first.size;
      const out = new Uint8Array(size);
      let done = Math.min(first.bytes.length, size);
      out.set(first.bytes.subarray(0, done), 0);
      onProgress?.(done, size);
      while (done < size) {
        const r = await read(() => s.read(path, done, Math.min(READ_MAX, size - done)));
        if (r.bytes.length === 0) throw new Error(`${path}: the device returned no data at ${done} of ${size} bytes`);
        const part = r.bytes.subarray(0, size - done);
        out.set(part, done);
        done += part.length;
        onProgress?.(done, size);
      }
      return out;
    },
    async verify(path) {
      const s = await session();
      return read(() => s.verify(path));
    },
  };
}

export function targetOps(
  app: Pick<TargetApp, 'bindSession'>,
  emulator?: Pick<RunOps, 'pause' | 'resume' | 'reset'>,
): TargetOps {
  const binding = app.bindSession();
  const actions = binding.target.kind === 'emulator' ? emulator : undefined;
  const run: RunOps = { why: (op) => (run[op] ? null : 'This operation is unavailable on this target.') };
  for (const op of ['pause', 'resume', 'reset'] as const) {
    const act = actions?.[op];
    if (act)
      run[op] = async () => {
        binding.check();
        await act();
      };
  }
  return { ...binding, store: sessionStoreOps(binding.session, binding.check), run };
}
