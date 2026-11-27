import { execFile, spawn } from 'node:child_process';
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createInterface } from 'node:readline';
import { promisify } from 'node:util';
import type { EmuRpcCall } from '../../packages/dk-session/src/index.ts';
import { wavBytes } from '../../packages/sharc-rig-kit/src/plot.ts';

interface CaptureSource {
  source: 'emulator-pcm' | 'usb-audio';
  route: string;
  input?: string;
  input_id?: string;
  wall_seconds?: number;
}

interface AudioReport {
  audio: { pcm_capture_path?: string; pcm_capture_error?: string } | null;
}

interface AudioInput {
  id: string;
  name: string;
}

type Capture = (file: string, play: () => Promise<void>) => Promise<CaptureSource>;

function requireBridge(binary: string) {
  if (!existsSync(binary)) throw new Error(`No dkaudio at ${binary} (run mise run dkaudio:build).`);
}

export async function listAudioInputs(binary: string): Promise<AudioInput[]> {
  requireBridge(binary);
  const { stdout } = await promisify(execFile)(binary, ['inputs'], { timeout: 10_000 });
  return JSON.parse(stdout);
}

/** Both sources record just the shared workload between opening and closing the stream. */
export function emulatorAudio(call: EmuRpcCall): Capture {
  return async (file, play) => {
    const { audio } = await call<AudioReport>('audio.report');
    if (!audio) throw new Error('The emulator has no DSP audio session.');
    if (audio.pcm_capture_path) throw new Error('An emulator PCM capture is already running.');
    await call('pcm.tap', { action: 'capture', path: file });
    try {
      await play();
      const { audio } = await call<AudioReport>('audio.report');
      if (audio?.pcm_capture_path !== file || audio.pcm_capture_error)
        throw new Error(audio?.pcm_capture_error || 'The emulator PCM capture was interrupted.');
    } finally {
      await call('pcm.tap', { action: 'capture_stop' });
    }
    return { source: 'emulator-pcm', route: 'master L/R' };
  };
}

export function usbAudio(binary: string, input: string): Capture {
  requireBridge(binary);
  return async (file, play) => {
    const pcm = file + '.f32le';
    const child = spawn(binary, ['capture', input, pcm], { stdio: ['pipe', 'pipe', 'inherit'], timeout: 20_000 });
    const closed = new Promise<string | null>((resolve) => {
      child.once('error', (e) => resolve(e.message));
      child.once('close', (code, signal) => resolve(code === 0 ? null : `exit ${code ?? signal}`));
    });
    child.stdin.on('error', () => {}); // The child can close the pipe on its own failure.
    const reader = createInterface({ input: child.stdout });
    const lines = reader[Symbol.asyncIterator]();
    try {
      const ready = await lines.next();
      if (ready.done) throw new Error(`USB audio did not start (${await closed})`);
      if (JSON.parse(ready.value).event !== 'ready') throw new Error('Unexpected USB audio readiness message.');
      await play();
      child.stdin.end('stop\n');
      const metadata = await lines.next();
      const error = await closed;
      if (error || metadata.done) throw new Error(`USB audio capture failed (${error ?? 'missing metadata'})`);
      const { frames, ...source } = JSON.parse(metadata.value) as CaptureSource & { frames: number };
      const raw = readFileSync(pcm);
      if (!Number.isSafeInteger(frames) || frames < 1 || raw.length !== frames * 8)
        throw new Error('USB audio bridge returned incomplete stereo PCM.');
      const samples = Array.from({ length: frames * 2 }, (_, i) => raw.readFloatLE(i * 4));
      writeFileSync(file, wavBytes(samples, 48_000, 2), { flag: 'wx' });
      return source;
    } finally {
      reader.close();
      child.kill(); // Only our capture helper, and a no-op if it already finished.
      await closed;
      rmSync(pcm, { force: true });
    }
  };
}
