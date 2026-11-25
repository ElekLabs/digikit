import {
  EventTimeout,
  matchesParameterChange,
  resolveParameterWatch,
  withParameterObserver,
  withViewObserver,
  type ParameterKey,
} from '@digi/dk-session';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { installWithDebug } from '@digi/regression/runtime/install.ts';
import { deviceProfile } from '@digi/regression/runtime/device-data.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { memory } from '@digi/regression/runtime/memory.ts';
import { sendCc } from '@digi/regression/runtime/midi.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';
import { readSmallRing, RING_SYMBOLS } from '@digi/regression/runtime/runtime-menu.ts';
import { prepareRuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { panelInput, scheduledTurns } from '@digi/test-support';
import { expect } from 'vitest';
import { loadDevices } from '../../../../../packages/devices/src/node.ts';
import { readStockDelayState } from '../../../../../packages/workbench-core/src/reference/stock-delay-state.ts';

runtimeTests('parameter observer follows a sample pick and panel and MIDI Delay edits', async (rt) => {
  const { dk, session } = rt;
  await expectRuntimeReady(session);
  try {
    await installWithDebug(rt);
  } catch (error) {
    const plan = await prepareRuntimePlan(session, liveArtifacts(session, 'parameter-observer'), RING_SYMBOLS);
    const load = (await readSmallRing(session, plan)).filter(
      (e) => e.kind === LOG_KIND.MOD_LOAD || e.kind === LOG_KIND.MOD_NEEDS || e.flags & 1,
    );
    throw new Error(
      `${String(error)}; boot and error records ${JSON.stringify(load.map((e) => ({ kind: e.kind, args: e.args.map((a) => a | 0) })))}`,
      { cause: error },
    );
  }
  const info = await dk.info();
  const profile = loadDevices().get(info.port);
  const version = profile.acceptance!.firmware;
  const sample = resolveParameterWatch(profile, version, 'sampleSlot', { scope: 'track', track: 1 });
  const feedback = resolveParameterWatch(profile, version, 'delay.feedback', { scope: 'kit' });
  const mem = memory(session);
  // Independent emulator witness of the exact DT2 1.17 firmware state.
  const { fx } = deviceProfile(session.options.device);
  const actualSlot = () => mem.soundParam(0, 28);
  const actualFeedback = async () => mem.half((await mem.cf(fx.kit_ptr + 4)) + 2 * 35);
  const stockFx = async () => {
    const base = await mem.cf(fx.kit_ptr + 4);
    return Promise.all(Array.from({ length: 48 }, (_, i) => mem.half(base + 2 * i)));
  };
  const events = {
    listen: async () => {
      await rt.listen();
    },
  };
  const drained = { until: { drained: true } } as const;
  const until = (key: ParameterKey, generation: number, match: (raw: number) => boolean) => ({
    event: 'VALUE_CHANGED' as const,
    match: (e: Parameters<typeof matchesParameterChange>[0]) => matchesParameterChange(e, key, generation, match),
    withinMs: 5000,
  });

  await withParameterObserver(dk, events, sample, async (original, generation) => {
    expect(original).toBe(await actualSlot());
    const wanted = original === 1 ? 0 : 1;
    await rt.ui.tap('SRC', drained);
    await rt.ui.turn('ENC D', 1, drained);
    for (let index = 0; index < 16; index++) await rt.ui.tap('UP', drained);
    if (wanted === 1) await rt.ui.tap('DOWN', drained);
    await rt.ui.tap('YES', { until: until(sample, generation, (value) => value === wanted) });
    expect(await actualSlot()).toBe(wanted);
  });

  await rt.ui.tap('FX', drained);
  await rt.ui.tap('TRIG 1', drained);
  await withViewObserver(dk, events, async (before, generation) => {
    const panel = panelInput(session, rt.clock);
    await panel.button('FUNC', true);
    try {
      await rt.ui.wait({ drained: true });
      await rt.ui.tap('FX', {
        until: {
          event: 'VIEW_CHANGED',
          match: (event) => event.args[4] === generation && event.args[1] !== before.object,
          withinMs: 5000,
        },
      });
    } finally {
      await panel.button('FUNC', false);
      await rt.ui.wait({ drained: true });
    }
  });
  await withParameterObserver(dk, events, feedback, async (original, generation) => {
    expect(original).toBe(await actualFeedback());
    const before = await stockFx();
    try {
      await scheduledTurns(session, rt.clock).burst('ENC D', 10, original < 0x7f00 ? 4 : -4, 56);
      await rt.ui.wait(until(feedback, generation, (value) => value !== original));
    } catch (error) {
      const after = await stockFx();
      const changed = after.flatMap((value, word) =>
        value === before[word] ? [] : [{ word, old: before[word], value }],
      );
      const dir = liveArtifacts(session, 'parameter-observer');
      record(dir, 'delay-failure-frame', await session.client.framebufferGet({ hash: true, data: true }));
      throw new Error(`${String(error)}; raw FX deltas ${JSON.stringify(changed)}; diagnostics ${dir}`, {
        cause: error,
      });
    }
    expect((await dk.parameterObserver()).value).toBe(await actualFeedback());
  });

  // Discover the configured FX Control channel only on this isolated fixture.
  // A hardware scenario must use its explicitly configured channel.
  await withParameterObserver(dk, events, feedback, async (original, generation) => {
    const feedbackCc = 24; // DT2 1.17 manual, appendix B.9.
    const value = original < 0x4000 ? 91 : 17;
    let channel = 0;
    for (let candidate = 1; candidate <= 16; candidate++) {
      await sendCc(rt, candidate, feedbackCc, value);
      try {
        await rt.ui.wait({ ...until(feedback, generation, (v) => v !== original), withinMs: 250 });
        channel = candidate;
        break;
      } catch (error) {
        if (!(error instanceof EventTimeout)) throw error;
      }
    }
    expect(channel, 'no FX Control channel produced a change to the live feedback word').toBeGreaterThan(0);
    const first = (await dk.parameterObserver()).value;
    expect(first).toBe(await actualFeedback());
    await sendCc(rt, channel, feedbackCc, value === 91 ? 17 : 91);
    await rt.ui.wait(until(feedback, generation, (v) => v !== first));
    expect((await dk.parameterObserver()).value).toBe(await actualFeedback());
  });
  expect(await readStockDelayState(dk, profile, version)).toEqual({
    track: 1,
    sample_slot: await actualSlot(),
    delay_feedback_raw: await actualFeedback(),
  });
  await session.expectNoFault('stock parameter observation');
});
