#!/usr/bin/env node
// The two-worker browser session under Node (V8), on the DN2 ready inputs:
// the ColdFire core instance runs in this thread, the DSP core instance in a
// worker_threads worker, and DSPI2 frames cross a SharedArrayBuffer
// (packages/workbench-core/src/sab-link.ts, split-session.ts: the same code the browser
// workers run). Prints the CPU split and the PCM hash, which must equal
// tools/devtools/wasm_parity.mjs's for the same scenario length (`idle`).
//
//   node tools/devtools/wasm_split.mjs --core CORE.wasm --seconds 1 [--fast 256,64,0]
//
// CORE.wasm is the generic core built with `--features sharc,fast-wasm`
// (no firmware in it). Needs Node >= 22.6 (type stripping of the .ts files).
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { Worker, isMainThread, parentPort, workerData } from 'node:worker_threads';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../..');
const web = path.join(root, 'packages/workbench-core/src');
const { createDspSide, createCfSide } = {
  ...(await import(path.join(web, 'split-session.ts'))),
};
const { createLinkBuffer, LinkServer } = await import(path.join(web, 'sab-link.ts'));

if (!isMainThread) {
  // The DSP worker.
  const d = workerData;
  const chunks = [];
  const side = await createDspSide({
    wasm: d.wasm,
    image: d.image,
    state: d.state,
    link: d.link,
    fast: d.fast,
    onPcm: (pcm) => chunks.push(Buffer.from(pcm.buffer)),
  });
  parentPort.postMessage({ type: 'ready' });
  const t0 = performance.now();
  side.serve();
  const wallMs = performance.now() - t0;
  const pcm = Buffer.concat(chunks);
  parentPort.postMessage({
    type: 'done',
    stats: side.stats(),
    wallMs,
    pcmValues: pcm.length / 4,
    sha256: crypto.createHash('sha256').update(pcm).digest('hex'),
    ...(d.keepPcm ? { pcm: new Uint8Array(pcm) } : {}),
  });
  process.exit(0);
}

const args = {};
for (let i = 2; i < process.argv.length; i += 2) args[process.argv[i].slice(2)] = process.argv[i + 1];
const env = process.env;
const ready = path.resolve(
  root,
  env.DIGI_EMU_READY_DIR ??
    JSON.parse(fs.readFileSync(path.join(root, 'tools/emulator/ready_inputs.json'), 'utf8')).ready_dir,
);
const read = (p) => new Uint8Array(fs.readFileSync(p));
const wasm = read(args.core);
const syx = read(args.syx ?? env.DIGI_EMU_SYX ?? path.join(root, 'firmware', 'Digitone_II_OS1.11.syx'));
const image = read(path.join(ready, 'digi-audio-dn2-image.bin'));
const state = read(path.join(ready, 'digi-audio-m5.snap.dsp'));
const snapshot = read(path.join(ready, 'digi-audio-m5.snap'));
const seconds = Number(args.seconds ?? 1);
const fast = args.fast ? args.fast.split(',').map(Number) : undefined;
const FRAMES_PER_SECOND = 1500;

const link = createLinkBuffer();
const worker = new Worker(fileURLToPath(import.meta.url), {
  workerData: { wasm, image, state, link, fast },
  execArgv: process.execArgv,
});
const done = new Promise((resolve, reject) => {
  worker.on('message', (m) => {
    if (m.type === 'done') resolve(m);
  });
  worker.on('error', reject);
});
await new Promise((resolve, reject) => {
  worker.once('message', (m) => m.type === 'ready' && resolve());
  worker.once('error', reject);
});

const t0 = performance.now();
const cf = await createCfSide({ wasm, syx, snapshot, link });
const loadS = (performance.now() - t0) / 1000;
const target = Math.ceil(seconds * FRAMES_PER_SECOND);
const wall0 = performance.now();
while (cf.stats().frames < target) cf.step();
const wallMs = performance.now() - wall0;
new LinkServer(link).close();
const dsp = await done;
const s = cf.stats();
const audio = target / FRAMES_PER_SECOND;
const cfBusy = s.stepMs - s.waitedMs;
console.log(
  `two instances, ${audio.toFixed(2)} audio s, load ${loadS.toFixed(2)} s, wall ${(wallMs / 1000).toFixed(2)} s (${(wallMs / 1000 / audio).toFixed(2)} s per audio s)`,
);
console.log(
  `ColdFire worker: ${(cfBusy / 1000 / audio).toFixed(3)} CPU s per audio s (step ${(s.stepMs / 1000).toFixed(2)} s, blocked on the DSP ${(s.waitedMs / 1000).toFixed(2)} s, ${s.requests} link requests)`,
);
console.log(
  `DSP worker:      ${(dsp.stats.busyMs / 1000 / audio).toFixed(3)} CPU s per audio s (${dsp.stats.frames} frames)`,
);
if (dsp.stats.fast)
  console.log(
    `fast tier: ${dsp.stats.kernels.compiled} kernels, ${dsp.stats.kernels.compileMs.toFixed(0)} ms compile, ${(dsp.stats.kernels.wasmBytes / 1e6).toFixed(2)} MB wasm, ${dsp.stats.kernels.runs} runs`,
  );
console.log(`pcm values=${dsp.pcmValues} sha256=${dsp.sha256}`);
