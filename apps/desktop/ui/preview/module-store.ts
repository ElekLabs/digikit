import type { PreviewModulePages } from './module-pages';
// The simulated card behind a preview target's DkSession. Its bytes survive a MIDI reopen and are shared by Transfer and Files.
import {
  DkError,
  HEADER_BYTES,
  INFO_PROBE_TIMEOUT_MS,
  READ_MAX,
  type DkInfoFull,
  type DkModule,
  type DkSession,
  type ModuleState,
  fileHashHex,
} from '@digi/dk-session';
import { CAP, MAX_CHUNK, PROTO_VERSION } from '@digi/dk-session/gen_proto';
import type { DeviceProfile } from '@digi/workbench-core/data/device-profiles.ts';
import { headerInfo, modulePath, portPath, servicePath } from '@digi/workbench-core/modules/dktransfer.ts';
import { fakeDkm } from '@digi/workbench-core/target-ops/fake-store.ts';
import { noteActivity } from '@digi/workbench-core/transfer/activity.ts';
import type { PreviewLibrary } from './module-library';
import { deviceRevision, sim } from './sim';
import { parseDkmHeader } from '@digi/workbench-core/target-ops/store.ts';
import { packedVersion, previewRuntimeVersion } from './runtime-version';

const KiB = 1024;
const runtime = packedVersion(previewRuntimeVersion);
const caps = [
  CAP.LOG,
  CAP.FOLDERS,
  CAP.FILEHASH,
  CAP.READ,
  CAP.VERIFY,
  CAP.MODULES,
  CAP.LOADS,
  CAP.CAPACITY,
  CAP.RESTART,
  CAP.FW_ID,
].reduce<number>((mask, bit) => mask | (1 << bit), 0);
const leaf = (path: string) => path.slice(path.lastIndexOf('/') + 1).slice(0, 27);
const timeout = () => new DkError({ error: 'No reply from the device.', kind: 'timeout' });

function wait(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const abort = () => {
      clearTimeout(timer);
      reject(new DkError({ error: 'Cancelled.', kind: 'cancelled' }));
    };
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', abort);
      resolve();
    }, ms);
    if (signal.aborted) abort();
    else signal.addEventListener('abort', abort, { once: true });
  });
}

function bootModules(files: Map<string, Uint8Array>, selected: Map<string, Uint8Array | undefined>) {
  const states: ModuleState[] = [];
  const modules: DkModule[] = [];
  for (const [path, failing] of selected) {
    const bytes = files.get(path);
    if (!bytes) continue;
    const header = headerInfo(bytes);
    const active = !!header && bytes !== failing;
    states.push({
      index: states.length,
      name: leaf(path),
      state: active ? 'ACTIVE' : 'FAILED',
      result: active ? 0 : -20,
      detail: [0, 0],
    });
    if (active) {
      const view = new DataView(bytes.buffer);
      modules.push({
        index: states.length - 1,
        name: header.name,
        version: view.getUint32(0x34),
        needs: view.getUint32(0x30) & 0x7fffffff,
        cf_base: 0x47e8b000 + modules.length * 0x8000,
        cf_size: view.getUint32(0x64),
        dsp_off: 0,
        dsp_len: 0,
        rows: 1,
        types: [1],
        faults: 0,
        guard_off: false,
      });
    }
  }
  return { states, modules };
}

async function serviceAbi(library: PreviewLibrary, device: string) {
  for (const { path } of await library.library.scan(device)) {
    if (!path.endsWith('.dks')) continue;
    const bytes = new Uint8Array(await library.library.read(device, path));
    const abi = parseDkmHeader(bytes)?.svcAbi;
    if (abi) return abi;
  }
  return null;
}

async function seedStore(library: PreviewLibrary, profile: DeviceProfile, pages: PreviewModulePages) {
  const device = profile.short;
  const files = new Map<string, Uint8Array>();
  const selected = new Map<string, Uint8Array | undefined>();
  if (sim.dev !== 'empty') {
    const [outdated, installed] = library.modules(device);
    for (const { id, pack } of [installed, outdated].filter((m) => m !== undefined)) {
      const bytes = new Uint8Array(await library.library.read(device, pack.file));
      const header = headerInfo(bytes);
      if (!header) continue;
      const path = modulePath(id, header.group);
      const old = id === outdated?.id;
      if (old) {
        // The card has an older preview build of the same module that failed to load.
        const view = new DataView(bytes.buffer);
        view.setUint32(0x34, Math.max(0, view.getUint32(0x34) - 1));
      }
      files.set(path, bytes);
      selected.set(path, old ? bytes : undefined);
    }
    // Seed the real providers alongside the two demo modules, so Transfer and Files see the same card.
    if (outdated || installed) {
      const providers = [servicePath('machines')];
      if (installed) providers.push(servicePath('samples'));
      const firmware = profile.firmware.at(-1);
      if (firmware) providers.push(portPath(`${device}-${firmware.version}`));
      for (const path of providers) {
        const filename = path.slice(path.lastIndexOf('/') + 1);
        files.set(path, new Uint8Array(await library.library.read(device, `${device}/${filename}`)));
      }
    }
    files.set('modules/tool/old-experiment.dkm', fakeDkm({ name: 'old/experiment', size: 5 * KiB }));
    if (outdated) files.set(`data/${outdated.id}/settings.bin`, Uint8Array.of(0, 1, 0, 8));
    files.set('samples/new/', new Uint8Array());
    pages.seed(files, selected);
  }
  const abi = await serviceAbi(library, device);
  return { files, selected, abi, boot: bootModules(files, selected) };
}

export function createPreviewStores(library: PreviewLibrary, pages: PreviewModulePages) {
  const stores = new Map<string, { revision: number; store: ReturnType<typeof seedStore> }>();

  async function restart(key: string) {
    const entry = stores.get(key);
    if (!entry) return; // Its first access will boot the card.
    const card = await entry.store;
    if (stores.get(key) !== entry) return;
    for (const path of card.selected.keys()) {
      if (!card.files.has(path)) card.selected.delete(path);
    }
    card.boot = bootModules(card.files, card.selected);
    pages.boot(key, card.boot.modules);
  }

  function storeSession(session: DkSession, profile: DeviceProfile, key: string, noRuntime: boolean) {
    let pending = new AbortController();
    const cancel = session.cancelPending.bind(session);
    const limit = 82 * 1024;
    const store = () => {
      const current = stores.get(key);
      if (current?.revision === deviceRevision) return current.store;
      const next = seedStore(library, profile, pages).then((card) => {
        if (stores.get(key)?.store === next) pages.boot(key, card.boot.modules);
        return card;
      });
      stores.set(key, { revision: deviceRevision, store: next });
      void next.catch(() => {
        if (stores.get(key)?.store === next) stores.delete(key);
      });
      return next;
    };
    const getFile = async (path: string) => {
      const bytes = (await store()).files.get(path);
      if (!bytes || path.endsWith('/'))
        throw new DkError({ error: `No such file: ${path}`, kind: 'device', status: 'E_NOT_FOUND' });
      return bytes;
    };
    const used = async () =>
      Math.ceil([...(await store()).files.values()].reduce((sum, file) => sum + file.length, 0) / KiB);
    const read = async (path: string, offset: number, length: number) => {
      const bytes = await getFile(path);
      return { size: bytes.length, bytes: bytes.slice(offset, offset + Math.min(length, READ_MAX)) };
    };

    return {
      cancelPending() {
        pending.abort();
        pending = new AbortController();
        cancel();
      },
      async info(quick): Promise<DkInfoFull> {
        const signal = pending.signal;
        if (sim.dev === 'noreply') {
          await wait(quick ? INFO_PROBE_TIMEOUT_MS : 6000, signal);
          throw timeout();
        }
        if (noRuntime) throw timeout();
        if (sim.dev === 'error') throw new Error('The card is not mounted (preview: dev=error).');
        const steps = sim.dev === 'slow' ? 12 : 1;
        for (let i = 0; i < steps; i++) {
          await wait(steps === 1 ? 90 : 200, signal);
          noteActivity();
        }
        const { abi } = await store();
        return {
          protocol: PROTO_VERSION,
          runtime_version: runtime & 0xffff,
          runtime,
          caps: caps | (abi === null ? 0 : 1 << CAP.SVC_ABI),
          port: profile.short,
          free_bytes: (limit - (await used())) * KiB,
          max_chunk: MAX_CHUNK,
          safe_mode: false,
          upload_open: false,
          card_read_only: false,
          log_sink: false,
          fault_found: false,
          card_sectors: null,
          card_state: 1,
          card_guard: null,
          image_id: '0123456789abcdef',
          svc_abi: abi,
          fw_id: profile.firmware.at(-1)?.sha256.slice(0, 16) ?? null,
        };
      },
      async list(prefix = '') {
        return [...(await store()).files]
          .filter(([path]) => path.startsWith(prefix))
          .map(([path, bytes]) => ({
            path,
            size: bytes.length,
            hash: path.endsWith('/') ? '0'.repeat(64) : fileHashHex(bytes),
          }))
          .sort((a, b) => a.path.localeCompare(b.path));
      },
      read,
      async header(path) {
        return (await read(path, 0, HEADER_BYTES)).bytes;
      },
      async verify(path) {
        await wait(100, pending.signal);
        return fileHashHex(await getFile(path));
      },
      async put(path, bytes, progress) {
        const signal = pending.signal;
        const { files } = await store();
        const step = Math.max(1, Math.round(sim.speed * 1024 * KiB * 0.1));
        let sent = 0;
        do {
          await wait(100, signal);
          sent = Math.min(bytes.length, sent + step);
          noteActivity();
          progress?.(sent, bytes.length);
        } while (sent < bytes.length);
        files.set(path, bytes.slice());
      },
      async delete(path) {
        const signal = pending.signal;
        const { files } = await store();
        await wait(100, signal);
        files.delete(path);
      },
      async modules() {
        return (await store()).boot.modules;
      },
      async loadStates() {
        return (await store()).boot.states;
      },
      async capacity() {
        const {
          boot: { modules },
          selected,
        } = await store();
        const cap = (limit: number, used = 0) => ({ limit, used, largest: limit - used });
        return {
          STORE: cap(limit, await used()),
          MODULES: cap(8, modules.length),
          SELECTION: cap(8, selected.size),
          ARENA: cap(
            512 * KiB,
            modules.reduce((total, module) => total + module.cf_size, 0),
          ),
          STAGE: cap(256 * KiB),
          DSP_AREA: cap(512 * KiB),
          MACHINE_ROWS: cap(8, modules.length),
          VOICE_ROWS: cap(8, modules.length),
          PARAM_ROWS: cap(16),
          PAGE_ROWS: cap(8),
          FX_SLOTS: cap(3),
          DSP_COST: cap(92_796),
          DSP_VOICES: cap(16),
        };
      },
      async select(paths) {
        const card = await store();
        card.selected = new Map(paths.map((path) => [path, card.selected.get(path)]));
      },
      async restart() {
        const signal = pending.signal;
        await store();
        await wait(1200, signal);
        await restart(key);
      },
      async waitForRestart() {
        return (await store()).boot.modules;
      },
    } satisfies Partial<DkSession>;
  }
  return { storeSession, restart };
}
