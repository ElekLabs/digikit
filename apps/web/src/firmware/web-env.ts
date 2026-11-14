// The site's side of the Firmware page (env.ts): patchkit.wasm in a worker patches and checks the image (the same worker as the old Patch page),
// a second instance on the page's own thread drives the Rust sender, and a download link gives the .syx. The MIDI side is the Transfer
// pane's (../transfer/web-env.ts). Every URL goes through BASE_URL.
import { SHOW_EMULATOR } from '../../site.config.mjs';
import { handoffStore, putHandoff } from '../patch/handoff';
import type { PatchKitLike, PreparedFlash } from '../patch/patchkit-core';
import { senderKit } from '../patch/sender-kit';
import { createWebTransferApp, type WebTransfer } from '../transfer/web-env';
import { fetchWasmSource, type WasmSource } from '../wasm-source';
import { browserWorkers, type WorkerFactory, type WorkerPort } from '../workers';
import { createFirmwareApp, type FirmwareApp } from './app';
import type { FirmwareEnv } from './env';
import './firmware-site.css';

const BASE = import.meta.env.BASE_URL.replace(/\/?$/, '/');

interface Reply {
  type: string;
  id?: number;
  error?: string;
  prepared?: Omit<PreparedFlash, 'image'> & { image: ArrayBuffer };
}

/** What the site's Firmware environment takes: where its workers and wasm come from (a test gives fakes), and how the sender kit is made. */
export interface WebFirmwareDeps {
  workers?: WorkerFactory;
  wasm?: WasmSource;
  /** The base URL the patch worker fetches the wasm and the module bundle from. */
  base?: string;
  /** Makes the sender kit from the wasm bytes (default: `PatchKit.create`); a test gives a stub kit. */
  createKit?: (bytes: ArrayBuffer, bundle: string) => Promise<PatchKitLike>;
}

/** The site's Firmware environment: patchkit.wasm in a worker, a second instance for the sender, a download link. */
export function createWebFirmwareEnv({
  workers = browserWorkers(),
  base = BASE,
  wasm = fetchWasmSource(base),
  createKit,
}: WebFirmwareDeps = {}): FirmwareEnv {
  let worker: WorkerPort | undefined;
  let nextId = 1;
  const pending = new Map<number, { resolve: (r: Reply) => void; reject: (e: Error) => void }>();
  let ready: Promise<void> | undefined;

  function patchWorker(): Promise<WorkerPort> {
    if (!worker) {
      const w = workers.create('patch');
      worker = w;
      ready = new Promise<void>((resolve, reject) => {
        w.onmessage = ({ data }: { data: Reply }) => {
          if (data.type === 'ready') return resolve();
          const waiter = data.id === undefined ? undefined : pending.get(data.id);
          if (!waiter) return reject(new Error(data.error ?? 'The patch tool failed to start.'));
          pending.delete(data.id!);
          if (data.type === 'error') waiter.reject(new Error(data.error));
          else waiter.resolve(data);
        };
        w.onerror = (e) => reject(new Error(`The patch tool failed to start: ${e.message}`));
        w.postMessage({ type: 'init', base });
      });
      ready.catch(() => {
        worker = undefined;
        ready = undefined;
      });
    }
    return ready!.then(() => worker!);
  }

  /** The sender needs no firmware or modules: an empty bundle. */
  const kit = senderKit(wasm, createKit);

  const webFirmwareEnv: FirmwareEnv = {
    async prepare(stock, restore, set = '') {
      const w = await patchWorker();
      const id = nextId++;
      const reply = await new Promise<Reply>((resolve, reject) => {
        pending.set(id, { resolve, reject });
        const copy = stock.slice().buffer;
        w.postMessage({ type: 'prepare', id, firmware: copy, restore, set }, [copy]);
      });
      const p = reply.prepared!;
      return { ...p, image: new Uint8Array(p.image) };
    },
    async sender(total) {
      return (await kit()).sender(total);
    },
    async rpc(image, chunk) {
      return (await kit()).rpcFlash(image, chunk);
    },
    async saveImage(name, bytes) {
      const url = URL.createObjectURL(new Blob([bytes.slice().buffer], { type: 'application/octet-stream' }));
      const a = document.createElement('a');
      a.href = url;
      a.download = name;
      document.body.append(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 10_000);
      return name;
    },
    ...(SHOW_EMULATOR
      ? {
          async runInEmulator(name: string, device: string, bytes: Uint8Array) {
            await putHandoff(handoffStore(), { name, device, bytes: bytes.slice().buffer });
            location.assign(`${BASE}emulator/?handoff=1`);
          },
        }
      : {}),
    log(run) {
      (run.level === 'error' ? console.warn : console.info)(`[firmware] ${run.text}`, run.detail ?? '');
    },
  };
  return webFirmwareEnv;
}

/** The site's apps for the Firmware page: the Transfer app (the MIDI port) and the Firmware app on it. */
export interface WebFirmware extends WebTransfer {
  firmware: FirmwareApp;
}

export function createWebFirmwareApp(): WebFirmware {
  const web = createWebTransferApp();
  return { ...web, firmware: createFirmwareApp(createWebFirmwareEnv(), web.app) };
}
