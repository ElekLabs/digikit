/// <reference lib="webworker" />
// The DSP worker of a two-worker session: its own instance of the browser
// core, answering each DSPI2 frame the ColdFire worker sends over the
// SharedArrayBuffer link. `serve()` blocks this worker until the page closes
// the link; nothing else runs here (PCM goes straight to the audio worklet
// through `pcmPort`, statistics to the page as `stats` messages).
import { createDspSide, type FastDefault, type FastRegions } from './split-session.ts';
import { pcmMetrics } from './runtime/runtime-metrics';

export interface DspStart {
  type: 'start';
  /** The core compiled once by the page and shared with the ColdFire worker, else fetched from `coreUrl`. */
  module?: WebAssembly.Module;
  coreUrl?: string;
  image: Uint8Array;
  state: Uint8Array;
  link: SharedArrayBuffer;
  fast?: [number, number, number] | FastRegions | FastDefault;
  pcmPort: MessagePort;
}

self.onmessage = async ({ data }: MessageEvent<DspStart>) => {
  if (data.type !== 'start') return;
  try {
    let startedAt = 0;
    const side = await createDspSide({
      wasm: data.module ?? fetch(data.coreUrl!),
      image: data.image,
      state: data.state,
      link: data.link,
      fast: data.fast,
      statsEvery: 50,
      onPcm: (pcm) => data.pcmPort.postMessage(pcm, [pcm.buffer]),
      onStats: (stats) =>
        postMessage({ type: 'stats', ...stats, ...pcmMetrics(stats.pcmValues, performance.now() - startedAt) }),
    });
    postMessage({ type: 'ready' });
    startedAt = performance.now();
    side.serve();
    postMessage({ type: 'done', stats: side.stats() });
  } catch (error) {
    postMessage({ type: 'error', error: String(error) });
  }
};
