import { readFileSync } from 'node:fs';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { Sample, prepareUpload } from '@digi/dk-session/dksample';
import { openSamples } from '@digi/dk-session/samples';
import { button, repoRoot } from '@digi/test-support';
import { expect } from 'vitest';
import type { PatchedSession } from '../support.ts';
import { installWithDebug, liveArtifacts, panel, record, runtimeTests, settleUi } from './live.ts';

// End to end, DT2 1.17: the device-neutral runtime (API 1.5 track table), core/sample-pool (the borrowed stock pool), machine contract 1.2
// and digikit/spark playing a stereo sample that a fixture module selected by path (docs/design/sample-api-1.5.md).
//
// The sample files have a different sine on each channel, so the tones in the captured PCM say which source channel reached the output.
// Everything is measured on the coupled PCM of the stock chain (pcm.tap). With the pair gate set by core/dsp-voice the stock pan stage keeps
// the two voices apart: the pair is split (`expectSplitPair`); the other checks take the louder channel of each tone.

const RATE = 48000;
const FOLDER = 'sparkproof';
const TRACK = 0; // TRIG 1
const HANDLE = 1; // the first module with a track table (digikit/spark)
const SECONDS = 6;

/** A stereo 48 kHz PCM16 WAV: a sine of `left` Hz on L and of `right` Hz on R. 48 kHz keeps the converter exact. */
function stereoWav(seconds: number, left: number, right: number): Uint8Array {
  const frames = seconds * RATE;
  const pcm = Buffer.alloc(frames * 4);
  for (let n = 0; n < frames; n++) {
    pcm.writeInt16LE(Math.round(14000 * Math.sin((2 * Math.PI * left * n) / RATE)), n * 4);
    pcm.writeInt16LE(Math.round(14000 * Math.sin((2 * Math.PI * right * n) / RATE)), n * 4 + 2);
  }
  const h = Buffer.alloc(44);
  h.write('RIFF', 0);
  h.writeUInt32LE(36 + pcm.length, 4);
  h.write('WAVEfmt ', 8);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20);
  h.writeUInt16LE(2, 22);
  h.writeUInt32LE(RATE, 24);
  h.writeUInt32LE(RATE * 4, 28);
  h.writeUInt16LE(4, 32);
  h.writeUInt16LE(16, 34);
  h.write('data', 36);
  h.writeUInt32LE(pcm.length, 40);
  return Uint8Array.from(Buffer.concat([h, pcm]));
}

// ---- measurement on the captured float WAV -------------------------------------------------------------------------------------

interface Capture {
  l: Float32Array;
  r: Float32Array;
}

function readCapture(file: string): Capture {
  const wav = readFileSync(file);
  expect(wav.toString('ascii', 0, 4)).toBe('RIFF');
  expect(wav.readUInt16LE(20), 'IEEE float').toBe(3);
  expect(wav.readUInt16LE(22)).toBe(2);
  expect(wav.readUInt32LE(24)).toBe(RATE);
  const frames = Math.floor((wav.length - 44) / 8);
  const l = new Float32Array(frames);
  const r = new Float32Array(frames);
  for (let i = 0; i < frames; i++) {
    l[i] = wav.readFloatLE(44 + i * 8);
    r[i] = wav.readFloatLE(44 + i * 8 + 4);
  }
  return { l, r };
}

const peak = (x: Float32Array, from: number, to: number) => {
  let p = 0;
  for (let i = Math.max(0, from); i < Math.min(x.length, to); i++) p = Math.max(p, Math.abs(x[i]!));
  return p;
};

/** Longest run of frames in [from, last sound) where both channels stay under the threshold. */
function longestSilence(c: Capture, from: number, threshold: number): number {
  let best = 0;
  let run = 0;
  for (let i = Math.max(0, from); i < c.l.length; i++) {
    if (Math.abs(c.l[i]!) > threshold || Math.abs(c.r[i]!) > threshold) {
      best = Math.max(best, run);
      run = 0;
    } else run++;
  }
  return best;
}

/** First frame where either channel exceeds the threshold, or -1. */
function onset(c: Capture, threshold = 0.01): number {
  for (let i = 0; i < c.l.length; i++) if (Math.abs(c.l[i]!) > threshold || Math.abs(c.r[i]!) > threshold) return i;
  return -1;
}

/** Amplitude of the sine of `hz` in x over frames [from, to) (Goertzel). A source sine of 14000/32768 reaches about 0.04 after the stock gain. */
function tone(x: Float32Array, hz: number, from: number, to: number): number {
  const w = (2 * Math.PI * hz) / RATE;
  let re = 0;
  let im = 0;
  const end = Math.min(to, x.length);
  for (let i = Math.max(0, from); i < end; i++) {
    re += x[i]! * Math.cos(w * i);
    im -= x[i]! * Math.sin(w * i);
  }
  return (2 * Math.hypot(re, im)) / Math.max(1, end - from);
}

const AUDIBLE = 0.015;
const ABSENT = 0.004;

/** The louder channel holds each tone of `want` and none of `absent` over frames [from, to). Returns the measured levels. */
function expectTones(name: string, c: Capture, from: number, to: number, want: number[], absent: number[]) {
  const levels = Object.fromEntries([...want, ...absent].map((hz) => [hz, Math.max(tone(c.l, hz, from, to), tone(c.r, hz, from, to))]));
  for (const hz of want) expect(levels[hz]!, `${name}: ${hz} Hz in the louder channel (${JSON.stringify(levels)})`).toBeGreaterThan(AUDIBLE);
  for (const hz of absent) expect(levels[hz]!, `${name}: ${hz} Hz absent (${JSON.stringify(levels)})`).toBeLessThan(ABSENT);
  return levels;
}

/** The pair is split: the left source (`left` Hz) is in L and the right source (`right` Hz) is in R, each with the other tone at least 20 dB
 * lower in the same channel. The stock pan stage takes the stereo-pair path when the even voice's gate word is 1 (core/dsp-voice sets it;
 * finding 96). The 20 dB limit is 0.1 of the wanted tone. Measured: the other tone is 1e-8 (A) and 1.3e-4 (B, window leakage) against 0.076, that is 55 dB
 * and more down; a summed pair (the old behaviour) gives a ratio of 1. 20 dB leaves a margin of 35 dB for noise without hiding a partial leak. */
function expectSplitPair(name: string, c: Capture, from: number, to: number, left: number, right: number) {
  const measured = {
    lLeft: tone(c.l, left, from, to),
    lRight: tone(c.l, right, from, to),
    rLeft: tone(c.r, left, from, to),
    rRight: tone(c.r, right, from, to),
  };
  const text = JSON.stringify(measured);
  expect(measured.lLeft, `${name}: the left source is in L ${text}`).toBeGreaterThan(AUDIBLE);
  expect(measured.rRight, `${name}: the right source is in R ${text}`).toBeGreaterThan(AUDIBLE);
  expect(measured.lRight, `${name}: the right source is at least 20 dB down in L ${text}`).toBeLessThan(measured.lLeft * 0.1);
  expect(measured.rLeft, `${name}: the left source is at least 20 dB down in R ${text}`).toBeLessThan(measured.rRight * 0.1);
  return measured;
}

// ---- the device ----------------------------------------------------------------------------------------------------------------

/** The panel path of sample-playback.test.ts: the sole dynamic machine, row 7 of the DT2 machine list, on the selected track (TRIG 1). */
async function selectDynamicMachine({ session }: Pick<PatchedSession, 'session'>, artifacts: string): Promise<void> {
  const ui = panel(session);
  await ui.tap('NO');
  await session.client.inputButton(button('FUNC'), true);
  try {
    await ui.settle(5);
    await ui.tap('SRC');
  } finally {
    await session.client.inputButton(button('FUNC'), false);
  }
  await ui.settle(205);
  for (let i = 0; i < 16; i++) await ui.tap('UP');
  for (let i = 0; i < 7; i++) await ui.tap('DOWN');
  await ui.tap('YES', 300);
  await ui.tap('YES', 300);
  record(artifacts, 'machine-page', await session.client.framebufferGet({ hash: true, data: true }));
}

function trackApi({ dk }: PatchedSession) {
  async function select(file: string, track = TRACK) {
    const reply = await dk.request(0x3e, Buffer.concat([Buffer.from([HANDLE, track]), Buffer.from(file)]));
    return reply.status;
  }
  /** 0 unused, 1 queued, 2 loading, 3 ready, below 0 an error; the info words when ready. */
  async function status(track = TRACK) {
    const reply = await dk.request(0x3f, Buffer.from([HANDLE, track]));
    expect(reply.status, 'track status request').toBe(0);
    const b = Buffer.from(reply.body);
    return {
      state: b.readInt32BE(0),
      frames: b.length >= 20 ? b.readUInt32BE(4) : 0,
      rate: b.length >= 20 ? b.readUInt32BE(8) : 0,
      channels: b.length >= 20 ? b.readUInt32BE(12) : 0,
      token: b.length >= 20 ? b.readUInt32BE(16) : 0,
    };
  }
  /** Poll until the track reaches a terminal state (ready or an error); every distinct observation is kept. */
  async function settle(timeoutMs = 90_000) {
    const seen: { t: number; state: number; token: number }[] = [];
    const t0 = Date.now();
    for (;;) {
      const s = await status();
      if (!seen.length || seen.at(-1)!.state !== s.state || seen.at(-1)!.token !== s.token)
        seen.push({ t: Date.now() - t0, state: s.state, token: s.token });
      if (s.state === 3 || s.state < 0) return { ...s, seen, ms: Date.now() - t0 };
      if (Date.now() - t0 > timeoutMs) throw new Error(`track ${TRACK} did not settle: ${JSON.stringify(seen)}`);
      await delay(50);
    }
  }
  return { select, status, settle };
}

/** A note on TRIG 1 held while `during` runs; returns the captured PCM. The clock counts stereo frames since the capture began
 * (pcm.tap reports floats, two per frame). */
async function captureNote(
  { session }: Pick<PatchedSession, 'session'>,
  file: string,
  during: (clock: () => Promise<number>) => Promise<void>,
): Promise<Capture> {
  const tapReport = async () =>
    (await session.client.call('pcm.tap', { action: 'report' })) as { samples: number; window_rms: number[] };
  const floats = async () => (await tapReport()).samples;
  await session.client.call('pcm.tap', { action: 'start' });
  await session.client.call('pcm.tap', { action: 'capture', path: file });
  const base = await floats();
  const clock = async () => Math.floor(((await floats()) - base) / 2);
  let pressedAt = 0;
  try {
    // A one-shot keeps sounding after the key is released: wait until the output has been silent for 0.3 s, then press.
    await expect
      .poll(
        async () => {
          const windows = (await tapReport()).window_rms;
          return windows.length >= 3 && windows.slice(-3).every((v) => v < 1e-4);
        },
        { timeout: 40_000, interval: 200 },
      )
      .toBe(true);
    pressedAt = await clock();
    await session.client.inputButton(button('TRIG 1'), true);
    try {
      await during(clock);
    } finally {
      await session.client.inputButton(button('TRIG 1'), false);
    }
  } finally {
    await session.client.call('pcm.tap', { action: 'capture_stop' });
  }
  const capture = readCapture(file);
  const from = pressedAt - RATE / 5;
  const before = Math.max(peak(capture.l, from, pressedAt - 480), peak(capture.r, from, pressedAt - 480));
  expect(before, `silent before the press at frame ${pressedAt}`).toBeLessThan(0.002);
  return capture;
}

async function until(clock: () => Promise<number>, frames: number, timeoutMs = 120_000): Promise<void> {
  await expect.poll(clock, { timeout: timeoutMs, interval: 100 }).toBeGreaterThanOrEqual(frames);
}

// ---- the test ------------------------------------------------------------------------------------------------------------------

runtimeTests(
  'SPARK plays a user-selected stereo sample through the runtime track table and the borrowed stock pool',
  async (runtime) => {
    const { session, dk } = runtime;
    const artifacts = liveArtifacts(session, 'spark-dt2');
    const api = trackApi(runtime);
    const log: Record<string, unknown> = {};
    try {
      await settleUi(session);
      await installWithDebug(runtime, 'digikit/spark', 'test/track-select', 'test/stock-pool');
      await settleUi(session);
      expect((await dk.modules()).map((m) => m.name)).toEqual(
        expect.arrayContaining(['digikit/spark', 'test/track-select', 'test/stock-pool']),
      );
      await selectDynamicMachine(runtime, artifacts);

      // 1. Two stereo files on the card: A has L 440 Hz and R 660 Hz, B has L 330 Hz and R 495 Hz.
      const sample = await Sample.create(
        Uint8Array.from(readFileSync(path.join(repoRoot(), 'apps/web/public/dksample.wasm'))),
      );
      const backend = openSamples('stock-rpc', { session: dk, hashOf: (data) => sample.hash(data) });
      await backend.mkdir(FOLDER);
      const files = [
        prepareUpload(sample, 'a.wav', stereoWav(SECONDS, 440, 660)),
        prepareUpload(sample, 'b.wav', stereoWav(SECONDS, 330, 495)),
      ];
      const uploadStart = Date.now();
      for (const file of files) {
        expect(file.kind).toBe('converted');
        await backend.put(FOLDER, file);
        expect((await backend.verify(FOLDER, file)).ok, `${file.name} verifies on the card`).toBe(true);
      }
      log.uploadMs = Date.now() - uploadStart;

      // 2 and 3. Select A; the track goes from unused to ready with the file's native header.
      expect((await api.status()).state, 'the track starts unused').toBe(0);
      expect(await api.select(`/${FOLDER}/a`), 'select queues').toBe(0);
      const readyA = await api.settle();
      log.selectA = readyA;
      expect(readyA.state, `A reaches ready: ${JSON.stringify(readyA.seen)}`).toBe(3);
      expect(readyA.channels).toBe(2);
      expect(readyA.rate).toBe(RATE);
      expect(Math.abs(readyA.frames - SECONDS * RATE), `frames ${readyA.frames}`).toBeLessThanOrEqual(64);
      expect(readyA.token).not.toBe(0);

      // 4. A note right after READY (READY means the record is in the DSP): both source channels reach the output.
      const noteA = await captureNote(runtime, path.join(artifacts, 'note-a.wav'), async (clock) => {
        await until(clock, 1.5 * RATE);
      });
      const onsetA = onset(noteA);
      log.noteA = { onset: onsetA, frames: noteA.l.length };
      expect(onsetA, 'the note is audible').toBeGreaterThanOrEqual(0);
      const winA: [number, number] = [onsetA + 4800, onsetA + RATE];
      log.noteALevels = expectTones('A', noteA, ...winA, [440, 660], []);
      log.noteASplit = expectSplitPair('A', noteA, ...winA, 440, 660);
      await session.expectNoFault('first note');

      // 6. Select B while a note of A plays: the old note stops. B is not retriggered because the key is held.
      let readyAtB = 0;
      let stateB: Awaited<ReturnType<typeof api.settle>> | undefined;
      const reselect = await captureNote(runtime, path.join(artifacts, 'reselect.wav'), async (clock) => {
        await until(clock, RATE / 2);
        expect(await api.select(`/${FOLDER}/b`), 'select B queues').toBe(0);
        stateB = await api.settle();
        readyAtB = await clock();
        await until(clock, readyAtB + 1.5 * RATE);
      });
      log.reselect = { readyAtB, stateB, frames: reselect.l.length };
      expect(stateB?.state, `B reaches ready: ${JSON.stringify(stateB?.seen)}`).toBe(3);
      expect(stateB!.token, 'a new token').not.toBe(readyA.token);
      const onsetR = onset(reselect);
      expect(onsetR, 'A was playing before the select').toBeGreaterThanOrEqual(0);
      expectTones('before the select', reselect, onsetR + 4800, RATE / 2 - 2400, [440, 660], [330, 495]);
      const tailFrom = readyAtB + RATE / 4;
      const tailPeak = Math.max(peak(reselect.l, tailFrom, reselect.l.length), peak(reselect.r, tailFrom, reselect.r.length));
      log.reselectTail = { tailFrom, tailPeak };
      expect(reselect.l.length - tailFrom, 'the capture covers the tail').toBeGreaterThan(RATE);
      expect(tailPeak, 'the old note is silent after the select').toBeLessThan(0.002);
      await session.expectNoFault('reselect while playing');

      // B plays now (a new note).
      const noteB = await captureNote(runtime, path.join(artifacts, 'note-b.wav'), async (clock) => {
        await until(clock, 1.5 * RATE);
      });
      const onsetB = onset(noteB);
      expect(onsetB, 'B is audible').toBeGreaterThanOrEqual(0);
      const winB: [number, number] = [onsetB + 4800, onsetB + RATE];
      log.noteBLevels = expectTones('B', noteB, ...winB, [330, 495], [440, 660]);
      log.noteBSplit = expectSplitPair('B', noteB, ...winB, 330, 495);

      // 7. The stock reset (ReloadAllSamples with 1) wipes the pool; the runtime reloads the track from the remembered path.
      const before = await api.status();
      expect(before.state).toBe(3);
      const trace: { t: number; state: number; token: number }[] = [];
      const noteReset = await captureNote(runtime, path.join(artifacts, 'reset.wav'), async (clock) => {
        await until(clock, RATE / 2);
        const t0 = Date.now();
        const reply = await dk.request(0x3a, Buffer.from([1]));
        expect(reply.status, 'stock-pool reload command').toBe(0);
        // The wipe comes about 1.5 s after the call; the track must leave the old binding, then be ready again.
        for (;;) {
          const s = await api.status();
          if (!trace.length || trace.at(-1)!.state !== s.state || trace.at(-1)!.token !== s.token)
            trace.push({ t: Date.now() - t0, state: s.state, token: s.token });
          if (s.state === 3 && s.token !== before.token) break;
          if (Date.now() - t0 > 120_000) throw new Error(`track did not reload: ${JSON.stringify(trace)}`);
          await delay(50);
        }
        await until(clock, (await clock()) + RATE);
      });
      log.reset = { before, trace, frames: noteReset.l.length };
      const after = await api.status();
      expect(after.state, `ready again after the reset: ${JSON.stringify(trace)}`).toBe(3);
      expect(after.token, 'the reload published a new token').not.toBe(before.token);
      expect(after.frames).toBe(before.frames);
      expect(after.channels).toBe(2);
      expect(
        trace.some((s) => s.state !== 3 || s.token !== before.token),
        `the track left the old binding: ${JSON.stringify(trace)}`,
      ).toBe(true);
      // The note that played over the reset ended with the old token. It would run for 6 s, so the capture must end well before that.
      const onsetS = onset(noteReset);
      expect(onsetS, 'B played before the reset').toBeGreaterThanOrEqual(0);
      expectTones('before the reset', noteReset, onsetS + 4800, RATE / 2 - 2400, [330, 495], [440, 660]);
      // The old note stops at the wipe and the track plays again after the reload: a silent gap lies inside the capture (measured: 2.3 s).
      const gap = longestSilence(noteReset, onsetS, 0.002);
      log.resetGap = gap;
      expect(gap, 'the note of the old token stopped for a while at the wipe').toBeGreaterThan(RATE / 2);
      const endPeak = Math.max(
        peak(noteReset.l, noteReset.l.length - RATE / 2, noteReset.l.length),
        peak(noteReset.r, noteReset.r.length - RATE / 2, noteReset.r.length),
      );
      log.resetTail = { endPeak };
      expect(endPeak, 'the note of the old token stopped').toBeLessThan(0.002);

      const noteAfter = await captureNote(runtime, path.join(artifacts, 'after-reset.wav'), async (clock) => {
        await until(clock, 1.5 * RATE);
      });
      const onsetAfter = onset(noteAfter);
      expect(onsetAfter, 'plays again after the reload').toBeGreaterThanOrEqual(0);
      log.noteAfterLevels = expectTones('after the reset', noteAfter, onsetAfter + 4800, onsetAfter + RATE, [330, 495], [440, 660]);

      // 5. Pitch: PITCH is 0.375 semitone per unit, so +32 units are one octave (ENC F: PITCH is knob 5, the second row of the SRC page under START; knob 3 is the sample slot).
      const ui = panel(session);
      for (let i = 0; i < 32; i++) await ui.turn('ENC F', 1);
      const noteUp = await captureNote(runtime, path.join(artifacts, 'octave.wav'), async (clock) => {
        await until(clock, 1.5 * RATE);
      });
      const onsetUp = onset(noteUp);
      expect(onsetUp, 'the note is audible').toBeGreaterThanOrEqual(0);
      log.octaveLevels = expectTones('one octave up', noteUp, onsetUp + 4800, onsetUp + RATE, [660, 990], [330, 495]);

      const modules = await dk.modules();
      log.modules = modules.map((m) => ({ name: m.name, faults: m.faults, off: m.guard_off }));
      for (const m of modules) expect(m.faults, `${m.name} faults`).toBe(0);
      await session.expectNoFault('SPARK stereo sample');
    } finally {
      record(artifacts, 'measurements', log);
      console.info('spark-dt2 measurements', JSON.stringify(log));
      try {
        record(artifacts, 'runtime-log', await dk.readLog());
      } catch {
        // The log is a diagnostic only.
      }
    }
  },
  { devices: ['dt2'], patchSet: 'sample-tracks', timeout: 1_200_000 },
);
