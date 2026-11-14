/// <reference lib="webworker" />
// The ColdFire worker of a two-worker session: its own instance of the generic
// core, stepping the firmware and blocking on the DSP worker at every DSPI2
// frame (the SharedArrayBuffer link), like the native `ThreadedPeer`. Panel
// input arrives as messages between steps.
import { createCfSide, type CfSide } from './split-session.ts';

export interface CfStart {
  type: 'start';
  coreUrl: string;
  syx: Uint8Array;
  /** The ColdFire snapshot at ready from the ready-state cache; empty: cold boot. */
  snapshot: Uint8Array;
  link: SharedArrayBuffer;
}

type Input =
  | { type: 'button'; code: number; down: boolean }
  | { type: 'turn'; encoder: number; detents: number }
  | { type: 'pause' }
  | { type: 'resume' }
  | { type: 'stats' };

const SLICE_MS = 20; // run steps for about this long, then look at the message queue
let side: CfSide | undefined;
let running = false;

function pump() {
  if (!side || !running) return;
  const begin = performance.now();
  try {
    do side.step();
    while (performance.now() - begin < SLICE_MS);
    postMessage({ type: 'status', ...side.stats(), leds: side.leds() });
  } catch (error) {
    running = false;
    postMessage({ type: 'error', error: String(error) });
    return;
  }
  setTimeout(pump, 0);
}

self.onmessage = async ({ data }: MessageEvent<CfStart | Input>) => {
  if (data.type === 'start') {
    try {
      side = await createCfSide({ wasm: fetch(data.coreUrl), syx: data.syx, snapshot: data.snapshot, link: data.link });
      running = true;
      postMessage({ type: 'ready' });
      pump();
    } catch (error) {
      postMessage({ type: 'error', error: String(error) });
    }
    return;
  }
  if (!side) return;
  if (data.type === 'button') side.core.call('digi_button', data.code, data.down ? 1 : 0);
  else if (data.type === 'turn') side.core.call('digi_turn', data.encoder, data.detents);
  else if (data.type === 'pause') running = false;
  else if (data.type === 'resume' && !running) {
    running = true;
    pump();
  } else if (data.type === 'stats') postMessage({ type: 'stats', ...side.stats() });
};
