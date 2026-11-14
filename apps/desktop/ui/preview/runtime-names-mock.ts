// The names of the fake emulator in the preview (log=crash): the app's own `createEmulatorNames` and dkprof.wasm over a fake Host whose controller
// answers `runtime.maps` for a small ship map and one module map; the load table is read through PEEK of the fake session (runtime-log-mock.ts).
import dkprofUrl from '../../../web/public/dkprof.wasm?url';
import { createDkProfLoader } from '../src/dkprof-env';
import { createFakeHost } from '@digi/workbench-core/host/fake-host.ts';
import { createEmulatorNames } from '../src/log-names-backend';
import { PREVIEW_STATE } from './runtime-log-mock';

const entry = (addr: number, size: number, name: string, module?: string) => ({
  cpu: 'cf',
  addr,
  size,
  name,
  layer: 'our',
  ...(module ? { module } : {}),
});
const ship = {
  format: 'dk-addrmap',
  version: 1,
  kind: 'ship',
  key: { device: 'dt2', loader: { modules: 2, l2_byte_base: 0x20000000, l2_sw_base: 0xb80000, dsp: false } },
  entries: [entry(0x40101000, 0x80, 'dkrt::tick')],
};
const moduleMap = {
  format: 'dk-addrmap',
  version: 1,
  kind: 'module',
  key: { module: 'test/dyn-demo' },
  entries: [entry(0, 0x40, 'init', 'test/dyn-demo'), entry(0x40, 0x40, 'render', 'test/dyn-demo')],
};
const host = createFakeHost({
  'rpc.call': async () => ({
    image_sha256: 'preview',
    matches: true,
    ship: JSON.stringify(ship),
    symbols: { DKRT_STATE: PREVIEW_STATE },
  }),
  'library.readMaps': { ships: [], modules: [{ id: 'test/dyn-demo', json: JSON.stringify(moduleMap) }] },
  'firmware.dkprofWasm': async () => (await fetch(dkprofUrl)).arrayBuffer(),
});
export const previewNames = createEmulatorNames(host, createDkProfLoader(host));
