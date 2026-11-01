import path from 'node:path';
import { button } from '@digi/test-support';
import { describe, expect, test } from 'vitest';
import type { PatchedSession } from '../support.ts';
import {
  advance,
  analysisJson,
  bounded,
  dspBytes,
  dspWords,
  frames,
  FREE_PC,
  inspect,
  pickSine,
  rows,
  rpc,
  withGainFxSession,
} from './fx-live.ts';
import { FIRMWARE, record } from './live.ts';
import { planSymbol } from './runtime-plan.ts';

const analyzer = 'tools.patchkit.pk_digikit_fxsend';
// Sine list position from the legacy DN2 fxsend scenario.
const sineHint = 5;

/** Preserve the legacy release-tail chord and no-note capture timing. */
async function captureSend(
  runtime: PatchedSession,
  file: string,
  chord: boolean,
  slotBus: number,
  artifacts: string,
  tag: string,
): Promise<void> {
  const { session } = runtime;
  const until = (target: number) => bounded(() => session.client.runUntil({ frames: target, timeout_wall_s: 30 }));
  const trigger = (down: boolean) => bounded(() => session.client.inputButton(button('TRIG 1'), down));
  await rpc(session, 'run.mode', { mode: 'paused' });
  await rpc(session, 'pcm.tap', { action: 'start' });
  await pickSine(session, sineHint);
  const trig = (await frames(session)) + 300;
  try {
    await until(trig);
    if (chord) await bounded(() => session.client.step(44_000, { timeout_wall_s: 30 }));
    await rpc(session, 'pcm.tap', { action: 'capture', path: file });
    if (chord) {
      await trigger(true);
      await until(trig + 60);
      await trigger(false);
      await until(trig + 120);
      await trigger(true);
      await until(trig + 220);
      const bus = await dspBytes(session, slotBus, 16 * 256);
      const peaks = Array.from({ length: 16 }, (_, slot) => {
        let peak = 0;
        for (let sample = 0; sample < 64; sample++) {
          const value = bus.readFloatLE(slot * 256 + sample * 4);
          expect(Number.isFinite(value), 'voice bus sample is finite').toBe(true);
          peak = Math.max(peak, Math.abs(value));
        }
        return peak;
      });
      const sounding = peaks.filter((peak) => peak > 1e-5).length;
      record(artifacts, `${tag}-voice-slots`, { peaks, sounding });
      expect(sounding, 'two notes of track 1 sound in at least two voice slots').toBeGreaterThanOrEqual(2);
    }
    await until(trig + (chord ? 1500 : 750));
  } finally {
    try {
      await rpc(session, 'pcm.tap', { action: 'capture_stop' });
    } finally {
      await trigger(false);
    }
  }
  await advance(session, 50);
  await bounded(() => session.expectNoFault(tag));
}

describe(`DN2 ${FIRMWARE.dn2} FX send voice slots`, () => {
  const audible = new WeakSet<PatchedSession>();
  const context = withGainFxSession('dn2', 'FX send DN2 chord/idle', {
    initial: 'wet',
    requireAudible: (runtime) => audible.has(runtime),
  });

  test('sums a release-tail chord and leaves free voice slots silent on the send route', async () => {
    const { fixture, plan, slots, artifacts, wet, rememberEpoch } = context;
    const { address: slotBus } = await analysisJson<{ address: number }>(
      analyzer,
      ['slot-bus', '--device', 'dn2', '--firmware', FIRMWARE.dn2],
      artifacts,
      'send-slot-bus',
    );
    const files: Record<string, string> = {};
    const initial = fixture.runtime;
    let previous = initial;
    for (const tag of ['chordzero', 'chordfull', 'idle'] as const) {
      const runtime = tag === 'chordzero' ? initial : await fixture.reload(wet);
      expect(runtime.session, 'all cases keep the owned daemon').toBe(initial.session);
      if (tag !== 'chordzero') expect(runtime.epoch).toBeGreaterThan(previous.epoch);
      previous = runtime;
      const state = await inspect(runtime, plan, slots, true, artifacts, `${tag}-initial`);
      rememberEpoch(state.epoch);
      const row = state.chain[0]!;
      expect(row[0], 'row 0 holds the effect').not.toBe(FREE_PC);
      expect(row[3], 'effect is enabled').toBe(1);
      expect(row.slice(18, 23), 'clean master route and zero amounts').toEqual([0, 0, 0, 0, 0]);
      const amounts = tag === 'idle' ? Array(4).fill(0x7f7f7f7f) : [tag === 'chordfull' ? 0x7f : 0, 0, 0, 0];
      const words: number[] = [1, ...amounts];
      const base = planSymbol(plan, 'fx.rows');
      await rpc(runtime.session, 'dsp.patch', {
        text: words
          .map((value, i) => `DM ${(base + 4 * (18 + i)).toString(16)} ${value.toString(16).padStart(8, '0')}\n`)
          .join(''),
      });
      expect((await rows(runtime.session, plan, slots))[0]!.slice(18, 23), 'send route and amounts read back').toEqual(
        words,
      );
      const file = path.join(artifacts, `${tag}.wav`);
      files[tag] = file;
      if (tag !== 'idle') audible.add(runtime);
      await captureSend(runtime, file, tag !== 'idle', slotBus, artifacts, tag);
      const after = (await rows(runtime.session, plan, slots))[0]!;
      expect(after[12], 'send adapter ran the effect').toBe(1);
      expect(after[14], 'effect remains enabled').toBe(0);
      expect(after[17], 'no effect fault').toBe(0);
      const n0 = (await dspWords(runtime.session, after[2]!, 1))[0]!;
      await bounded(async () =>
        runtime.session.client.runUntil({ frames: (await frames(runtime.session)) + 100, timeout_wall_s: 30 }),
      );
      const n1 = (await dspWords(runtime.session, after[2]!, 1))[0]!;
      record(artifacts, `${tag}-after-note`, { row: after, n0, n1 });
      expect(n0, 'effect call counter has advanced').toBeGreaterThan(1000);
      if (tag !== 'idle') {
        expect(n1 - n0, 'send effect runs once per block').toBeGreaterThanOrEqual(90);
        expect(n1 - n0).toBeLessThanOrEqual(110);
      } else {
        // Legacy idle checks call activity but deliberately does not constrain the delta.
        const pcm = await analysisJson<{ samples: number; finite: boolean; peak: number }>(
          analyzer,
          ['pcm', file],
          artifacts,
          'idle-pcm',
        );
        expect(pcm.samples).toBeGreaterThan(1000);
        expect(pcm.finite).toBe(true);
        expect(pcm.peak, 'free slots contribute no signal').toBe(0);
      }
    }
    const factor = await analysisJson<{ added_factor: number }>(
      analyzer,
      ['added-factor', files.chordzero!, files.chordfull!],
      artifacts,
      'chord-added-factor',
    );
    expect(Math.abs(factor.added_factor / 0.5 - 1), 'send return carries half the sum of the chord slots').toBeLessThan(
      0.001,
    );
  }, 300_000);
});
