// dkprof.wasm in the workbench: the Rust code that names addresses and decodes the load table (packages/dkprof), loaded through the Host
// like patchkit.wasm. Loaded once, when the first name is needed; a failed load is tried again at the next use. Web is not in scope.
import { DkProf } from '@digi/dk-session/dkprof';
import type { Host } from './host/host';

export function createDkProfLoader(host: Pick<Host, 'firmware'>): () => Promise<DkProf> {
  let loaded: Promise<DkProf> | undefined;
  return () => {
    loaded ??= host.firmware
      .dkprofWasm()
      .then((bytes) => DkProf.create(bytes))
      .catch((e: unknown) => {
        loaded = undefined;
        throw e;
      });
    return loaded;
  };
}
