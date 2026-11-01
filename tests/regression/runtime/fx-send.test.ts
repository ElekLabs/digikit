import path from 'node:path';
import { describe, expect, test } from 'vitest';
import { DEVICES, FIRMWARE, record } from './live.ts';
import { planSymbol } from './runtime-plan.ts';
import {
  analysisJson,
  bounded,
  capture,
  compare,
  dspWords,
  frames,
  inspect,
  rows,
  rpc,
  withGainFxSession,
} from './fx-live.ts';

for (const device of DEVICES) {
  describe(`${device.toUpperCase()} ${FIRMWARE[device]} FX SEND`, () => {
    const context = withGainFxSession(device, 'FX SEND dry/zero/other/full/half');
    test('isolates common send amounts and preserves the squared send gain law', async () => {
      const { fixture, plan, wet, slots, artifacts, rememberEpoch } = context;
      // Legacy fx-send-{dt2,dn2}.toml list positions. This is the accepted audible-input
      // contract; direct selected-type observations remain in the legacy basic dry/full legs.
      const hint = device === 'dt2' ? 7 : 5;
      const dry = path.join(artifacts, 'send-dry.wav');
      const initial = fixture.runtime;
      rememberEpoch((await inspect(initial, plan, slots, false, artifacts, 'send-dry')).epoch);
      await capture(initial, dry, hint);

      const pcm = new Map<string, string>();
      let previous = initial;
      for (const [tag, amount] of [
        ['zero', 0],
        ['other', 0x7f00],
        ['full', 0x7f],
        ['half', 0x40],
      ] as const) {
        const runtime = await fixture.reload(wet);
        expect(runtime.session).toBe(initial.session);
        expect(runtime.epoch).toBeGreaterThan(previous.epoch);
        await expect(previous.dk.info()).rejects.toThrow();
        previous = runtime;
        const state = await inspect(runtime, plan, slots, true, artifacts, `send-${tag}`);
        rememberEpoch(state.epoch);
        const before = state.chain[0]!;
        expect(before[0]).not.toBe(0xffffffff);
        expect(before[3], 'effect is enabled before routing').toBe(1);
        expect(before.slice(18, 23), 'default master route and all send amounts').toEqual([0, 0, 0, 0, 0]);
        const base = planSymbol(plan, 'fx.rows');
        await rpc(runtime.session, 'dsp.patch', {
          text: `DM ${(base + 18 * 4).toString(16)} 00000001\nDM ${(base + 19 * 4).toString(16)} ${amount.toString(16).padStart(8, '0')}\n`,
        });
        const routed = (await rows(runtime.session, plan, slots))[0]!;
        expect(routed.slice(18, 23), 'send route and packed track amounts read back').toEqual([1, amount, 0, 0, 0]);
        const file = path.join(artifacts, `send-${tag}.wav`);
        await capture(runtime, file, hint);
        pcm.set(tag, file);
        const after = (await rows(runtime.session, plan, slots))[0]!;
        expect(after[12], 'send effect ran').toBe(1);
        expect(after[14], 'send effect remains active').toBe(0);
        expect(after[17], 'send effect has no fault').toBe(0);
        const n0 = (await dspWords(runtime.session, after[2]!, 1))[0]!;
        await bounded(async () =>
          runtime.session.client.runUntil({ frames: (await frames(runtime.session)) + 100, timeout_wall_s: 30 }),
        );
        const n1 = (await dspWords(runtime.session, after[2]!, 1))[0]!;
        expect(n0).toBeGreaterThan(1000);
        expect(n1 - n0, 'send runs once per block, without a second master-chain call').toBeGreaterThanOrEqual(90);
        expect(n1 - n0).toBeLessThanOrEqual(110);
        record(artifacts, `send-${tag}-after-note`, { row: after, counts: [n0, n1] });
      }
      for (const tag of ['zero', 'other']) {
        await compare(dry, pcm.get(tag)!, 1, artifacts, `send-${tag}`, true);
      }
      const added = async (tag: string) => {
        const result = await analysisJson<{ added_factor: number }>(
          'tools.patchkit.pk_digikit_fxsend',
          ['added-factor', pcm.get('zero')!, pcm.get(tag)!],
          artifacts,
          `send-${tag}-added-factor`,
        );
        return result.added_factor;
      };
      const full = await added('full');
      const half = await added('half');
      expect(Math.abs(full / 0.5 - 1), 'full send adds half the input').toBeLessThan(0.001);
      expect(Math.abs(half / full / (64 / 127) ** 2 - 1), 'send amount follows the stock squared law').toBeLessThan(
        0.01,
      );
      // ZERO is independently bit-identical to DRY above; a nonzero added factor
      // against ZERO proves FULL differs from DRY without treating an expected
      // comparator mismatch (its nonzero CLI exit) as an analysis failure.
      expect(full, 'full return changes dry PCM').not.toBe(0);
    });
  });
}
