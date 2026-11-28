// Web worker: fetch the tools, build the .dkm from the .dkmod, report timings. Message in: { root, device }; message out: { log, ... }.
import { Fs, Patchkit, buildDkm, unbundle } from './pipeline.mjs';

const log = [];
const note = (step, ms, extra) => log.push({ step, ms: Math.round(ms), ...extra });
async function fetchBytes(url, label) {
  const t = performance.now();
  const r = await fetch(url);
  const b = new Uint8Array(await r.arrayBuffer());
  note(`fetch ${label}`, performance.now() - t, { bytes: b.length, wire: Number(r.headers.get('x-wire-bytes') ?? 0) });
  return b;
}

self.onmessage = async ({ data: { root, device, count, modBase } }) => {
  try {
    const tools = {};
    for (const n of ['cc1', 'as', 'ld', 'nm', 'objcopy']) {
      const b = await fetchBytes(`/a/${n}.wasm`, `${n}.wasm`);
      const t = performance.now();
      tools[n === 'as' ? 'as' : n] = await WebAssembly.compile(b);
      note(`compile ${n}.wasm`, performance.now() - t, {});
    }
    const pk = await fetchBytes('/a/patchkit.wasm', 'patchkit.wasm');
    const patchkit = new Patchkit((await WebAssembly.instantiate(pk, {})).instance);
    const fs = new Fs();
    const inputs = await fetchBytes('/inputs.bin', 'include+sdk bundle');
    const t = performance.now();
    const n = unbundle(inputs, fs);
    note('unpack bundle', performance.now() - t, { files: n });
    const sha = async (b) =>
      [...new Uint8Array(await crypto.subtle.digest('SHA-256', b))]
        .map((x) => x.toString(16).padStart(2, '0'))
        .join('');
    const modules = [];
    for (let i = 0; i < count; i++) {
      const dkmod = await fetchBytes(`/input-${i}.dkmod`, `.dkmod ${i}`);
      const mark = log.length;
      const t0 = performance.now();
      let r;
      try {
        r = await buildDkm({ tools, patchkit, fs, dkmod, root, device, log: note, modBase });
      } catch (e) {
        modules.push({ error: String(e?.message ?? e).slice(0, 400), steps: log.slice(mark) });
        continue;
      }
      note('build total (after fetches)', performance.now() - t0, {});
      modules.push({
        id: r.id,
        ms: Math.round(performance.now() - t0),
        dkm: r.dkm,
        dko: r.dko,
        sha256: await sha(r.dkm),
        peakWasmMem: r.peakWasmMem,
        steps: log.slice(mark),
      });
    }
    self.postMessage({ ok: true, log, modules });
  } catch (e) {
    self.postMessage({ ok: false, error: String(e?.stack ?? e), log });
  }
};
