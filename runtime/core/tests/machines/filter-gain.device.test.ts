// device-literals: allow
import path from 'node:path';
import { expect, test } from 'vitest';
import { record } from '@digi/regression/runtime/artifacts.ts';
import { bootInfo } from '@digi/regression/runtime/card-kit.ts';
import { cardRigs, RT } from '@digi/regression/runtime/card-rig.ts';
import { deviceProfile } from '@digi/regression/runtime/device-data.ts';
import { SINE } from '@digi/regression/runtime/fixtures.ts';
import { captureNote, frames, peak, pickSine, readFloatWav, REDRAW, windowRms, word, type Rt } from '@digi/regression/runtime/fx-live.ts';
import { halfword, selectTrack } from '@digi/regression/runtime/fx-track-ui.ts';
import { dynType, selectDt2 } from '@digi/regression/runtime/project-kit.ts';
import { turnPastLimit } from './machines-kit.ts';

const FILTER = 'test/filter-gain';
const TRACK = 1;
// The fixture is the only module filter on a fresh card. It follows the six stock filters (findings 148 and 158).
const STOCK_FILTER_COUNT = 6;
const FILTER_GAIN_TYPE = 16;
const GAIN_MAX = 0x7f00;
// Track Sound objects start their machine and filter bytes at these offsets (finding 148).
// The GAIN descriptor inherits the mirror index of the second stock filter knob (finding 158).
const LAYOUT = {
  dt2: { typeOffset: 0xa2, gainIndex: 0x25 },
  dn2: { typeOffset: 0xde, gainIndex: 0x44 },
};

async function note(rt: Rt, artifacts: string, tag: string) {
  const file = path.join(artifacts, `${tag}.wav`);
  const trigger = (await frames(rt.session)) + 60;
  await captureNote(rt.session, { file, trigger, tag, length: 450 });
  // The capture is interleaved stereo at 48 kHz. Skip the onset, then check three consecutive 50 ms windows.
  const pcm = readFloatWav(file);
  const windowSamples = 4800;
  expect(pcm.length, `${tag} captured more than 200 ms of stereo PCM`).toBeGreaterThan(4 * windowSamples);
  const settled = pcm.subarray(windowSamples, 4 * windowSamples);
  expect(settled.every(Number.isFinite), `${tag} PCM is finite`).toBe(true);
  return { peak: peak(settled), rms: windowRms(pcm.subarray(windowSamples), windowSamples).slice(0, 3) };
}

cardRigs(
  'module filter gain',
  {
    devices: ['dt2', 'dn2'],
    dsp: 'sharc',
    roots: RT,
    hold: true,
    initial: ({ mod }) => ({
      modules: [SINE, FILTER],
      selection: [mod(SINE).name, mod(FILTER).name],
    }),
  },
  (rig) => {
    test('Filter Gain on track 1 mutes Sine at zero and restores steady audio at full gain', async () => {
      const rt = await rig.first();
      const { session } = rt;
      const boot = await bootInfo(session, rig.plan);
      expect({ safe: boot.safe, results: boot.results.slice(0, boot.count) }, 'Sine and Filter Gain loaded').toEqual({
        safe: 0,
        results: [0, 0],
      });

      const { fx } = deviceProfile(rig.device);
      const { typeOffset, gainIndex } = LAYOUT[rig.device as keyof typeof LAYOUT];
      const sound = (await word(session, fx.kit_ptr)) + fx.sound_off + (TRACK - 1) * fx.sound_stride;
      const types = () => halfword(session, sound + typeOffset);
      const sineType = await dynType(session, rig.plan, SINE, 'sine');
      await selectTrack(rt, TRACK);
      if (rig.device === 'dt2') await selectDt2(rt, TRACK, sineType, 7);
      else await pickSine(rt);

      const before = await types();
      expect(before >>> 8, 'track 1 has the Sine machine').toBe(sineType);
      expect(before & 0xff, 'track 1 starts with a stock filter').toBeLessThan(STOCK_FILTER_COUNT);
      const dry = await note(rt, rig.artifacts, 'stock-filter');
      expect(dry.peak, 'Sine through the stock filter is audible').toBeGreaterThan(0.01);
      await rt.ui.wait({ ms: 2000, why: 'the fixture voice has a two-second lifetime' });

      await rt.ui.tap('FLTR', { until: { ms: 200, why: REDRAW } });
      // SRC on DT2 and SYN on DN2 share the same panel key code.
      await rt.ui.tap('SRC', { held: 'FUNC', until: { ms: 200, why: REDRAW } });
      // The chooser starts on the active stock filter. UP reaches its first row.
      for (let i = 0; i < STOCK_FILTER_COUNT; i++) await rt.ui.tap('UP', { until: { ms: 80, why: REDRAW } });
      for (let i = 0; i < STOCK_FILTER_COUNT; i++) await rt.ui.tap('DOWN', { until: { ms: 80, why: REDRAW } });
      await rt.ui.tap('YES', {
        until: {
          state: types,
          done: (value) => value === ((sineType << 8) | FILTER_GAIN_TYPE),
          withinMs: 1000,
          what: 'track 1 keeps Sine and commits Filter Gain',
        },
      });
      await rt.ui.tap('NO', { until: { ms: 200, why: REDRAW } });

      expect(await turnPastLimit(rt, 'ENC F', gainIndex, -4, 0), 'GAIN reaches zero').toBe(0);
      const muted = await note(rt, rig.artifacts, 'zero-gain');
      expect(muted.peak, 'Filter Gain mutes the Sine note').toBeLessThan(dry.peak * 0.001);
      // Each phase triggers one mono Sine note after the prior voice expires, including on DN2.
      await rt.ui.wait({ ms: 2000, why: 'the fixture voice has a two-second lifetime' });

      expect(await turnPastLimit(rt, 'ENC F', gainIndex, 4, GAIN_MAX), 'GAIN reaches full scale').toBe(GAIN_MAX);
      const restored = await note(rt, rig.artifacts, 'full-gain');
      record(rig.artifacts, 'filter-gain-levels', { dry, muted, restored });
      expect(Math.min(...restored.rms), 'each restored window is audible').toBeGreaterThan(dry.peak * 0.1);
      expect(Math.max(...restored.rms) / Math.min(...restored.rms), 'the restored level stays steady').toBeLessThan(1.25);
    }, 900_000);
  },
);
