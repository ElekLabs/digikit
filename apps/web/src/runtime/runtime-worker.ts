/// <reference lib="webworker" />

import { hostMetrics, pcmMetrics } from '@digi/workbench-core/runtime/runtime-metrics.ts';
import { LinkClient } from '@digi/workbench-core/sab-link.ts';
const metrics = hostMetrics('wasm_abi');
const base = import.meta.env.BASE_URL.replace(/\/?$/, '/');

type Abi = Record<string, CallableFunction>;
let wasm: Abi | undefined;
let plain: Abi | undefined;
let corePromise: Promise<Abi> | undefined;
let sharcPromise: Promise<Abi> | undefined;
let capablePromise: Promise<boolean> | undefined;
let generation = 0;
let runEpoch = 0;
let running = false;
let serial = Promise.resolve();
// Coupled ColdFire + SHARC+ audio (dev path, `sharc` build of the core).
let coupled = false;
// Two-worker session: this worker is the ColdFire; the DSP instance runs in
// dsp-worker.ts and answers each DSPI2 frame over a SharedArrayBuffer link.
let client: LinkClient | undefined;
let audioPort: MessagePort | undefined;
let pcmValues = 0;
let audioStartedAt = 0;
let lastIcount = 0;
const pendingRelease: { code: number; at: number }[] = [];
// A coupled step runs ~3 DSP frames, so loop steps inside one task (setTimeout
// would clamp to 4 ms between them) and yield about every 20 ms for input.
const COUPLED_SLICE_MS = 20;
// Re-arm through a MessageChannel: it queues a task without the 4 ms clamp
// that nested setTimeout(0) calls get, and worker messages still interleave.
const yieldChannel = new MessageChannel();
const yieldQueue: (() => void)[] = [];
yieldChannel.port1.onmessage = () => {
  yieldQueue.shift()?.();
};
function yieldThen(fn: () => void) {
  yieldQueue.push(fn);
  yieldChannel.port2.postMessage(0);
}

function result() {
  if (!wasm) throw new Error('WASM core is not loaded');
  const pointer = wasm.digi_result_ptr() as number;
  const length = wasm.digi_result_len() as number;
  return JSON.parse(
    new TextDecoder().decode(new Uint8Array((wasm.memory as unknown as WebAssembly.Memory).buffer, pointer, length)),
  );
}
function call(name: string, ...args: number[]) {
  if (!wasm) throw new Error('WASM core is not loaded');
  const code = wasm[name](...args) as number;
  const value = result();
  if (code !== 0 || value.error) throw new Error(value.error ?? `native error ${code}`);
  return value.snapshot ?? value;
}
function stopCore() {
  if (!wasm) return;
  wasm.digi_stop();
  result();
  coupled = false;
  client = undefined;
  pendingRelease.length = 0;
}
function memory() {
  return (wasm!.memory as unknown as WebAssembly.Memory).buffer;
}
/** Move the PCM produced since the last call to the audio port. */
function sendPcm() {
  const count = wasm!.digi_pcm_take() as number;
  if (count === 0 || !audioPort) return;
  const block = new Float32Array(memory(), wasm!.digi_pcm_ptr() as number, count).slice();
  pcmValues += count;
  audioPort.postMessage(block, [block.buffer]);
}
/** On-demand observational report; no stepping or new device reads. */
function coupledReport() {
  if (client)
    return {
      cf_icount: lastIcount,
      split: { link_wait_ms: client.waitedMs, link_requests: client.requests, link_frames: client.frames },
      dsp_attached: true,
      pcm_port_connected: Boolean(audioPort),
      host: metrics.report(),
    };
  return {
    ...call('digi_sharc_stats'),
    cf_icount: lastIcount,
    pcm_values: pcmValues,
    ...pcmMetrics(pcmValues, performance.now() - audioStartedAt),
    dsp_attached: coupled,
    pcm_port_connected: Boolean(audioPort),
    host: metrics.report(),
  };
}
/** Copy a page-supplied buffer into the module; returns its pointer. */
function put(bytes: Uint8Array) {
  const pointer = wasm!.digi_alloc(bytes.length) as number;
  if (pointer === 0 && bytes.length !== 0) throw new Error('native allocation failed');
  new Uint8Array(memory(), pointer, bytes.length).set(bytes);
  return pointer;
}
function core() {
  corePromise ??= WebAssembly.instantiateStreaming(fetch(`${base}emulator-core.wasm`), {}).then(({ instance }) => {
    plain = instance.exports as unknown as Abi;
    wasm ??= plain;
    return plain;
  });
  return corePromise;
}
/** The coupled core is a separate, large file, fetched only when the audio
 * panel loads a coupled session. */
function sharcCore() {
  sharcPromise ??= WebAssembly.instantiateStreaming(fetch(`${base}emulator-core-sharc.wasm`), coreImports).then(
    ({ instance }) => instance.exports as unknown as Abi,
  );
  return sharcPromise;
}
/** Imports the coupled core declares: the fast tier's kernel host (unused here) and the remote DSP link. */
const coreImports = {
  env: {
    host_wasm_compile: () => -1,
    host_wasm_run: () => 0,
    host_wasm_free() {},
    host_dspi_exchange(tx: number, len: number, rx: number) {
      if (!client) throw new Error('host_dspi_exchange: this instance has no remote DSP');
      const reply = client.exchange(new Uint8Array(memory(), tx, len).slice());
      new Uint8Array(memory(), rx, len).set(reply);
    },
    host_dspi_link(pointer: number, len: number) {
      if (!client) throw new Error('host_dspi_link: this instance has no remote DSP');
      client.link(new Uint8Array(memory(), pointer, len).slice());
    },
  },
};
/** Cheap availability probe: does not download the large file. */
function coupledCapable() {
  capablePromise ??= fetch(`${base}emulator-core-sharc.wasm`, { method: 'HEAD' }).then(
    (response) => response.ok,
    () => false,
  );
  return capablePromise;
}
function publish(snapshot: unknown, token: number) {
  postMessage({ type: 'snapshot', generation: token, snapshot });
}
function step() {
  const snapshot = call('digi_step', 250_000);
  lastIcount = snapshot.status.icount;
  if (coupled && !client) sendPcm();
  for (let i = pendingRelease.length - 1; i >= 0; i -= 1) {
    if (snapshot.status.icount >= pendingRelease[i].at) {
      call('digi_button', pendingRelease[i].code, 0);
      pendingRelease.splice(i, 1);
    }
  }
  return snapshot;
}
async function pump(token: number, epoch: number) {
  if (!running || token !== generation || epoch !== runEpoch) return;
  try {
    const begin = performance.now();
    let snapshot = step();
    metrics.step(begin, performance.now(), snapshot);
    // `leds` is sent only when it changed, so an intermediate step's value must survive to the published one.
    let leds = snapshot.leds,
      rgb = snapshot.rgb;
    while (coupled && !snapshot.status?.error && performance.now() - begin < COUPLED_SLICE_MS) {
      const t = performance.now();
      snapshot = step();
      metrics.step(t, performance.now(), snapshot);
      if (snapshot.leds !== undefined) {
        leds = snapshot.leds;
        rgb = snapshot.rgb;
      }
    }
    if (leds !== undefined) {
      snapshot.leds = leds;
      snapshot.rgb = rgb;
    }
    publish(snapshot, token);
    if (snapshot.status?.error) {
      running = false;
      postMessage({ type: 'error', generation: token, error: snapshot.status.error });
      return;
    }
  } catch (error) {
    running = false;
    postMessage({ type: 'error', generation: token, error: String(error) });
    return;
  }
  yieldThen(() => {
    void pump(token, epoch);
  });
}
function reject(data: { id?: string }, error: string) {
  if (data.id) postMessage({ reply: data.id, error });
}
type Message = {
  type: string;
  link?: SharedArrayBuffer;
  module?: WebAssembly.Module;
  id?: string;
  generation: number;
  bytes?: ArrayBuffer;
  image?: ArrayBuffer;
  dsp?: ArrayBuffer;
  snapshot?: ArrayBuffer;
  port?: MessagePort;
  hold?: number;
  code?: number;
  down?: boolean;
  encoder?: number;
  detents?: number;
};
async function handle(data: Message) {
  if (data.type === 'audio-port') {
    audioPort = data.port;
    return;
  }
  if (data.type === 'coupled-capable') {
    const value = await coupledCapable();
    if (data.id) postMessage({ reply: data.id, value });
    return;
  }
  if (data.type === 'load') {
    if (data.generation < generation) return reject(data, 'stale emulator session');
    generation = data.generation;
    running = false;
    runEpoch += 1;
    stopCore();
    wasm = await core();
    if (!data.bytes) return reject(data, 'firmware bytes are missing');
    const bytes = new Uint8Array(data.bytes);
    const pointer = wasm!.digi_alloc(bytes.length) as number;
    if (pointer === 0 && bytes.length !== 0) return reject(data, 'native allocation failed');
    try {
      new Uint8Array((wasm!.memory as unknown as WebAssembly.Memory).buffer, pointer, bytes.length).set(bytes);
      metrics.reset();
      const snapshot = call('digi_load', pointer, bytes.length);
      metrics.loaded();
      publish(snapshot, generation);
      running = true;
      const epoch = ++runEpoch;
      yieldThen(() => {
        void pump(generation, epoch);
      });
      if (data.id) postMessage({ reply: data.id, value: snapshot });
    } finally {
      wasm!.digi_dealloc(pointer, bytes.length);
    }
    return;
  }
  if (data.type === 'load-coupled') {
    if (data.generation < generation) return reject(data, 'stale emulator session');
    generation = data.generation;
    running = false;
    runEpoch += 1;
    stopCore();
    const split = Boolean(data.link && data.module);
    try {
      wasm = split
        ? ((await WebAssembly.instantiate(data.module!, coreImports)).exports as unknown as Abi)
        : await sharcCore();
    } catch (error) {
      wasm = plain;
      return reject(data, `Audio setup needs a browser core built with SHARC support: ${error}`);
    }
    if (typeof wasm.digi_load_coupled !== 'function') {
      wasm = plain;
      return reject(data, 'Audio setup needs a browser core built with SHARC support.');
    }
    if (split && typeof wasm.digi_load_coupled_remote !== 'function') {
      wasm = plain;
      return reject(data, 'This browser core has no two-instance support.');
    }
    if (!data.bytes || (!split && (!data.image || !data.dsp)))
      return reject(data, 'firmware, DSP image and DSP state are required');
    const parts = split
      ? [new Uint8Array(data.bytes), new Uint8Array(data.snapshot ?? new ArrayBuffer(0))]
      : [
          new Uint8Array(data.bytes),
          new Uint8Array(data.image!),
          new Uint8Array(data.dsp!),
          new Uint8Array(data.snapshot ?? new ArrayBuffer(0)),
        ];
    const pointers: number[] = [];
    try {
      for (const part of parts) pointers.push(put(part));
      metrics.reset();
      if (split) client = new LinkClient(data.link!);
      const snapshot = call(
        split ? 'digi_load_coupled_remote' : 'digi_load_coupled',
        ...parts.flatMap((part, i) => [pointers[i], part.length]),
      );
      metrics.loaded();
      coupled = true;
      pcmValues = 0;
      audioStartedAt = performance.now();
      publish(snapshot, generation);
      running = true;
      const epoch = ++runEpoch;
      yieldThen(() => {
        void pump(generation, epoch);
      });
      if (data.id) postMessage({ reply: data.id, value: snapshot });
    } finally {
      parts.forEach((part, i) => {
        if (i < pointers.length) wasm!.digi_dealloc(pointers[i], part.length);
      });
    }
    return;
  }
  if (data.type === 'stop') {
    if (data.generation < generation) return reject(data, 'stale emulator session');
    generation = data.generation;
    running = false;
    runEpoch += 1;
    stopCore();
    if (data.id) postMessage({ reply: data.id, value: undefined });
    return;
  }
  if (data.generation !== generation) return reject(data, 'stale emulator session');
  if (data.type === 'coupled-stats') {
    if (!coupled) return reject(data, 'not a coupled session');
    if (data.id) postMessage({ reply: data.id, value: coupledReport() });
    return;
  }
  if (data.type === 'tap') {
    call('digi_button', data.code!, 1);
    pendingRelease.push({ code: data.code!, at: lastIcount + (data.hold ?? 40_000_000) });
    if (data.id) postMessage({ reply: data.id, value: undefined });
    return;
  }
  if (data.type === 'diagnostics') {
    const report = {
      ...call('digi_diagnostics'),
      host: metrics.report(),
      audio: coupled ? coupledReport() : undefined,
    };
    if (data.id) postMessage({ reply: data.id, value: report });
    return;
  }
  if (data.type === 'pause') {
    running = false;
    runEpoch += 1;
    return;
  }
  if (data.type === 'resume') {
    if (!running) {
      running = true;
      const epoch = ++runEpoch;
      yieldThen(() => {
        void pump(generation, epoch);
      });
    }
    return;
  }
  if (data.type === 'button') {
    call('digi_button', data.code!, data.down ? 1 : 0);
    if (data.id) postMessage({ reply: data.id, value: undefined });
    return;
  }
  if (data.type === 'turn') {
    call('digi_turn', data.encoder!, data.detents!);
    if (data.id) postMessage({ reply: data.id, value: undefined });
    return;
  }
}
self.onmessage = ({ data }) => {
  serial = serial
    .then(() => handle(data))
    .catch((error) => reject(data, error instanceof Error ? error.message : String(error)));
};
