import path from 'node:path';
import type { DkSession } from '@digi/dk-session';
import { encoder } from '@digi/test-support';
import { describe, expect, test } from 'vitest';
import { withWaitApi } from '@digi/regression/wait-api.ts';
import { type TestRuntime } from '@digi/regression/test-runtime.ts';
import { rootsOf } from '@digi/regression/runtime/image-roots.ts';
import { bounded, rpc, word } from '@digi/regression/runtime/guest-access.ts';
import { captureNote, pausedTrigger } from '@digi/regression/runtime/note-capture.ts';
import { compare, pcmMetrics } from '@digi/regression/runtime/pcm-files.ts';
import { rows } from '@digi/regression/runtime/dsp-state.ts';
import { REDRAW, SETTLE } from '@digi/regression/runtime/panel-waits.ts';
import { GAIN, SINE, triggerFrame } from '@digi/regression/runtime/fixtures.ts';
import { createFxKitUi, watchFxKitRows, type FxKitObservation } from '@digi/regression/runtime/fx-kit-ui.ts';
import { bits, mainPage, openPages, pageKit } from '@digi/regression/runtime/fx-pages-ui.ts';
import {
  inspectFxBoot,
  selectSine,
  shot,
  withPausedFxProtocol,
  withPreparedFxSession,
} from '@digi/regression/runtime/fx-session.ts';
import { halfword, trackNote } from '@digi/regression/runtime/fx-track-ui.ts';
import { record } from '@digi/regression/runtime/artifacts.ts';
import { DEVICES, FIRMWARE } from '@digi/regression/runtime/devices.ts';
import { expectRuntimeUi } from '@digi/regression/runtime/ready.ts';
import { symbolsOf } from '@digi/regression/runtime/service-symbols.ts';
import { emulatorDk } from '@digi/regression/runtime/waits.ts';

// The effect pages' reaction in the firmware's kit and project paths, in two tests per device:
// 1. the signed knob of test/dyn-fx-gain-bipolar: the row words after a turn, the MIX endpoints, kit save and load, one PCM compare of the gain;
// 2. the kit and project values of test/dyn-fx-gain: round trips, another pattern's kit object, no transient zero in a load, the storage record the hooks write.
// The host proves the rest: the page row, the strings and the parameter records, the signed knob's raw range with its default in the middle, the defaults and ids
// tables of a reorder (services/fx/tests/contract.rs), the send amounts (fx-track-route and fxsend/tests.rs), and the gain law of the bias and of MIX (the rig of
// tests/fixtures/modules/dyn-fx-gain-bipolar).

const BIPOLAR = 'test/dyn-fx-gain-bipolar';
const BIPOLAR_ROUTE = `fx/route/${BIPOLAR}`;
const biasFloat = (raw: number) => Math.fround(-64 + Math.fround(127 * Math.fround(raw / 32512)));
const gainOf = (bias: number) => Math.fround(1 + bias / 64);
const floatWord = (raw: number) => {
  const bytes = Buffer.alloc(4);
  bytes.writeUInt32LE(raw);
  return bytes.readFloatLE();
};

for (const device of DEVICES) {
  describe(`${device.toUpperCase()} ${FIRMWARE[device]} FX contract, signed knob`, () => {
    const context = withPreparedFxSession(device, 'FX contract bipolar', {
      image: {
        name: 'fx-bipolar',
        modules: ['core/runtime', 'core/rows', 'core/dyndsp', 'core/dynfx'],
        inventory: [SINE, BIPOLAR],
        symbols: ['DKRT_BOOT', 'fx.rows', 'params.fp0.index', 'pf_table'],
        // The kit library and the runtime settings are written, as in the route and kit tests.
        policy: 'fx-routes',
      },
      initial: 'dry',
      presets: {
        dry: { selection: ['modules/m0.dkm'] },
        // The fixture starts on the send route; the master is stored.
        master: { selection: ['modules/m0.dkm', 'modules/m1.dkm'], settings: [[BIPOLAR_ROUTE, '00']] },
      },
      // The dry amplitude and the relative PCM comparison below own audibility.
      requireAudible: false,
    });

    test('a turn of the signed knob reaches the row, MIX ends at its limits, a kit round trip restores both, and the gain reaches the PCM', async () => {
      const { fixture, plan, profile, artifacts, slots } = context;
      let runtime = fixture.runtime;
      const pcm: Record<string, string> = {};
      const row = async () => (await rows(runtime.session, plan, slots))[0]!;
      const words = async () => {
        const r = await row();
        return [r[4], r[13]];
      };
      const boot = async (tag: string, names: string[], reload = true) => {
        if (reload) {
          const previous = runtime;
          runtime = await fixture.reload(context.baseline(tag));
          expect(runtime.session).toBe(previous.session);
          expect(runtime.epoch).toBeGreaterThan(previous.epoch);
          await expect(previous.dk.info()).rejects.toThrow();
        }
        const state = await inspectFxBoot(runtime, plan, names, artifacts, `${tag}-boot`);
        context.rememberEpoch(state.epoch);
        return state;
      };
      const sound = async (tag: string, initial = false) => {
        const { current, trigger: next } = await pausedTrigger(runtime.session);
        const trigger = initial ? triggerFrame(device) : next;
        record(artifacts, `${tag}-timing`, { currentFrame: current, triggerFrame: trigger });
        const file = path.join(artifacts, `${tag}.wav`);
        await captureNote(runtime.session, { file, trigger, tag: 'bipolar note', length: 1600 });
        pcm[tag] = file;
      };
      const noFault = async () => expect((await row())[17]).toBe(0);
      // The row words and the kit are firmware state: a knob change is waited for on them.
      const rowIs = (what: string, done: (r: number[]) => boolean) =>
        runtime.ui.wait({ state: row, done, withinMs: 5000, what });
      const NO_PAGE_STATE = 'no firmware word shows the FX page change (only the page list and its rows)';

      await boot('dry', [SINE], false);
      await selectSine(runtime, profile);
      await mainPage(runtime);
      await sound('dry', true);

      // Master route: the signed values and the PCM.
      await boot('master', [SINE, BIPOLAR]);
      await selectSine(runtime, profile);
      await openPages(runtime);
      for (let i = 0; i < 3; i++) await runtime.ui.tap('DOWN', { until: { drained: true } });
      await runtime.ui.wait({ ms: 600, why: NO_PAGE_STATE });
      await shot(runtime.session, artifacts, 'gbp-page-default');
      const kit = pageKit(runtime, profile);
      const ui = createFxKitUi(runtime, plan, profile, artifacts);
      // the defaults the page seeds: BIAS 0 is raw 0x4000 in the middle of the range (host: the signed knob unit), and the row words follow
      expect([await kit.raw(0), await kit.raw(7)]).toEqual([0x4000, 0x7f00]);
      expect(await words()).toEqual([0, bits(1)]);
      await mainPage(runtime);

      // The MIX endpoints: approach, sixty fast events, settle.
      const endpoint = async (near: number, direction: number, target: number, tag: string) => {
        await ui.fxPage(3);
        await kit.setRaw('LEVEL', 7, near);
        for (let i = 0; i < 60; i++) {
          await bounded(() => runtime.session.client.inputTurn(encoder('LEVEL'), direction * 8));
          await runtime.ui.wait({ ms: 2, why: 'fast encoder events: the legacy burst spacing' });
        }
        await runtime.ui.wait({
          state: () => kit.raw(7),
          done: (v: number) => v === target,
          withinMs: 5000,
          what: 'the MIX knob is at its end',
        });
        expect(await kit.raw(7)).toBe(target);
        await rowIs('the MIX word at target', (r) => r[13] === (target ? bits(1) : 0));
        expect((await words())[1]).toBe(target ? bits(1) : 0);
        await shot(runtime.session, artifacts, tag);
      };
      const bias: Record<string, number> = {};
      const turn = async (raw: number, tag: string) => {
        await ui.fxPage(3);
        const got = await kit.setRaw('ENC A', 0, raw);
        const value = biasFloat(got);
        await rowIs('the SHIFT word follows the knob', (r) => r[4] === bits(value));
        await shot(runtime.session, artifacts, `gbp-page-bias-${tag}`);
        expect(await words()).toEqual([bits(value), bits(1)]);
        bias[tag] = value;
      };
      await turn(0x6800, 'up');
      await mainPage(runtime);
      await sound('c');
      // MIX bypass after a nonzero bias: a MIX that fails to bypass shows as gain.
      await endpoint(0x1000, -1, 0, 'gbp-page-mix-zero');
      await mainPage(runtime);
      await endpoint(0x7000, 1, 0x7f00, 'gbp-page-mix-top');
      await mainPage(runtime);
      await ui.fxPage(3);
      const halfRaw = await kit.setRaw('LEVEL', 7, 0x4000);
      await rowIs('the MIX word follows the knob', (r) => Math.abs(floatWord(r[13]!) - halfRaw / 0x7f00) < 1e-4);
      expect(Math.abs(floatWord((await row())[13]!) - halfRaw / 0x7f00)).toBeLessThan(1e-4);
      await mainPage(runtime);
      await endpoint(0x7000, 1, 0x7f00, 'gbp-page-mix-top-again');
      await mainPage(runtime);
      await turn(0x1800, 'down');
      await mainPage(runtime);
      await noFault();
      const saved = [await kit.raw(0), await kit.raw(7)];
      await mainPage(runtime);
      await ui.saveKit(2, 'gbp');
      await ui.fxPage(3);
      await kit.setRaw('ENC A', 0, 0x6800);
      await mainPage(runtime);
      const upRaw = await kit.raw(0);
      const loaded = await ui.loadKit(2, 'gbp');
      record(artifacts, 'gbp-load-observations', loaded);
      expect([await kit.raw(0), await kit.raw(7)]).toEqual(saved);
      expect(upRaw).not.toBe(saved[0]);
      expect(await words()).toEqual([bits(bias.down!), bits(1)]);
      await ui.fxPage(3);
      await runtime.ui.wait({ ms: 300, why: NO_PAGE_STATE });
      await shot(runtime.session, artifacts, 'gbp-page-loaded');
      await mainPage(runtime);
      await noFault();

      // One end-to-end PCM compare is the boundary proof: the dry PCM of the first card, scaled by the gain of the float word that the row holds. The gain
      // law itself, the zero bias, MIX 0, 0.5 and 1, and other biases are proven on the SHARC rig (tests/fixtures/modules/dyn-fx-gain-bipolar/tests/rig).
      await compare(pcm.dry!, pcm.c!, gainOf(bias.up!), artifacts, 'bias-up');
    });
  });
}

const MARK = 0x46a7;
const defaultsOne = [0x3200, ...Array<null>(6).fill(null), 0x7f00];
const isDefault = (got: number[], want: (number | null)[]) =>
  want.every((value, i) => value === null || Math.abs(got[i]! - value) <= 1);
const expectedRecord = (ids: number[], values: number[], size: number) => {
  const result = ids.flatMap((id, row) => (id ? values.slice(row * 8, row * 8 + 8) : Array<number>(8).fill(0)));
  result.push(...ids);
  return [...result, ...Array<number>(size - result.length).fill(0)];
};

for (const device of DEVICES) {
  describe(`${device.toUpperCase()} ${FIRMWARE[device]} FX kit/project persistence`, () => {
    const full = device === 'dt2';
    const context = withPreparedFxSession(device, 'FX kit/project persistence', {
      image: {
        name: 'fx-kit-persist',
        modules: rootsOf('rt-dynfx'),
        inventory: [SINE, GAIN],
        symbols: ['DKRT_BOOT', 'fx.rows'],
        policy: 'fx-kit',
      },
      initial: full ? 'stock' : 'effects',
      presets: {
        stock: { selection: ['modules/m0.dkm'] },
        effects: { selection: ['modules/m0.dkm', 'modules/m1.dkm'] },
      },
      // The relative PCM comparisons below retain the legacy audibility oracle.
      requireAudible: false,
    });

    test('round-trips kit and project values, gives another pattern its own kit object, and never shows a transient zero in a load', async () => {
      const { fixture, plan, profile, artifacts } = context;
      let runtime: TestRuntime = fixture.runtime;
      const session = runtime.session;
      const fx = profile.fx;
      const count = fx.value_indices.length / 8;
      // the state of the hooks is in the fx service file: `symbolsOf` has its symbols after each boot (`inspectFxBoot`)
      const sym = (name: string) => symbolsOf(runtime, plan)(name);
      const cell = (name: string) => word(session, sym(name));
      const halfwords = async (base: number, n: number) =>
        Promise.all(Array.from({ length: n }, (_, i) => halfword(session, base + 2 * i)));
      const ids = () => halfwords(sym('fxp_ids'), count);
      const kit = pageKit(runtime, profile);
      const kitBase = async () => (await word(session, fx.kit_ptr)) + fx.array_off;
      const values = async () => {
        const base = await kitBase();
        return Promise.all(fx.value_indices.map((index) => halfword(session, base + 2 * index)));
      };
      const stock = async () => halfword(session, (await kitBase()) + 64);
      const mark = async () => halfword(session, (await kitBase()) + 2 * fx.mark_index);
      const spare = async (base: number) => halfwords(base + fx.store_off, fx.store_halfwords);
      const rowWords = async () => (await rows(session, plan, count)).slice(0, 1).map((row) => [row[4], row[13]]);
      const poke16 = async (at: number, value: number) => {
        for (let byte = 0; byte < 2; byte++) {
          const address = at + byte;
          const shift = 8 * (3 - (address % 4));
          const old = await word(session, address & ~3);
          await rpc(session, 'mem.cf.poke', {
            addr: address & ~3,
            value: ((old & ~(0xff << shift)) | (((value >>> (8 * (1 - byte))) & 0xff) << shift)) >>> 0,
          });
        }
      };
      const ui = () => createFxKitUi(runtime, plan, profile, artifacts);
      const noZero = (tag: string, seen: FxKitObservation[]) => {
        record(artifacts, `${tag}-row-transitions`, seen);
        expect(
          seen.filter(([, , p0, mix]) => p0 === 0 || mix === 0),
          `${tag}: no transient zero p0/mix`,
        ).toEqual([]);
      };
      const inspect = async (names: string[], tag: string, afterEpoch?: number) => {
        const boot = await inspectFxBoot(runtime, plan, names, artifacts, tag, undefined, { afterEpoch });
        context.rememberEpoch(boot.epoch);
      };
      const protocols: DkSession[] = [];
      // The card holds the machines service, so the safety policy allows the settings copies. Only the first boot that loads a machine
      // writes them (machines.types); a restart on the same card must not, because the type map is no longer dirty.
      const restart = async (selection: string[], tag: string, names: string[]) => {
        await context.evidence(runtime, `${tag}-before-restart`);
        await withPausedFxProtocol(runtime, (dk) => dk.select(selection));
        const epoch = (await bounded(() => session.status())).epoch;
        expect(typeof epoch).toBe('number');
        context.rememberEpoch(epoch);
        await rpc(session, 'run.mode', { mode: 'free' });
        await bounded(() => runtime.dk.restart());
        runtime.dk.cancelPending();
        const dk = emulatorDk(session);
        protocols.push(dk);
        runtime = withWaitApi({ session, dk, close: runtime.close });
        // Keep the original epoch for both barriers; neither silence nor a stale UI proves restart.
        await expectRuntimeUi(session, plan, artifacts, { afterEpoch: Number(epoch) });
        await bounded(() => session.client.guardSet(context.baseline('effects').guard));
        await inspect(names, `${tag}-boot`, Number(epoch));
      };
      const pcm = new Map<string, string>();
      const note = async (tag: string) => {
        await mainPage(runtime);
        const file = path.join(artifacts, `${tag}.wav`);
        await trackNote(runtime, file, (await pausedTrigger(session)).trigger);
        pcm.set(tag, file);
      };
      const ratio = (base: string, sample: string, gain: number, identical = false) =>
        compare(pcm.get(base)!, pcm.get(sample)!, gain, artifacts, sample, identical);
      const page = async (tag: string) => {
        await ui().fxPage(3);
        await runtime.ui.wait({ ms: 600, why: SETTLE });
        return shot(session, artifacts, tag);
      };
      try {
        let stockK1 = 0x4800;
        if (full) {
          await inspect([SINE], 'stock-boot');
          await selectSine(runtime, profile);
          const raw = await stock();
          await kit.poke(32, 0x4800);
          stockK1 = await stock();
          expect(stockK1).not.toBe(raw);
          // the stock boot has no fx service (no hook cells): only the stock kit values are checked
          await ui().saveKit(1, 'k1');
          await kit.poke(32, 0x2000);
          const moved = await stock();
          expect(moved).not.toBe(stockK1);
          await ui().loadKit(1, 'k1');
          expect(Math.abs((await stock()) - stockK1)).toBeLessThanOrEqual(0x40);
          expect(await stock()).not.toBe(moved);
          await restart(['modules/m0.dkm', 'modules/m1.dkm'], 'effects', [SINE, GAIN]);
        } else await inspect([SINE, GAIN], 'effects-boot');
        const effectIds = await ids();
        expect(effectIds[0]).toBe(0x4625);
        expect(effectIds.slice(1)).toEqual(Array(count - 1).fill(0));
        console.info(
          `fx service counters: ${(await Promise.all(['fxp_ninit', 'fxp_nload', 'fxp_nsave', 'fxp_nsound'].map(async (n) => `${n}=${await cell(n)}`))).join(' ')}`,
        );
        // the init hook is not observable in this test (boot init runs before fx.dks binds; Load Kit does not call the initialiser, finding 61); the service seeds a live kit on activation and tick
        expect(await mark()).toBe(MARK);
        expect(isDefault(await values(), defaultsOne)).toBe(true);
        await selectSine(runtime, profile);
        expect(isDefault(await values(), defaultsOne)).toBe(true);
        if (!full) {
          // the quick setup (no restart) emits the same zero-ID record as a sine-only boot.
          await kit.poke(32, stockK1);
          for (let row = 0; row < count; row++) await poke16(sym('fxp_ids') + row * 2, 0);
          try {
            await ui().saveKit(1, 'k1');
          } finally {
            for (let row = 0; row < count; row++) await poke16(sym('fxp_ids') + row * 2, effectIds[row]!);
          }
          expect(await spare(await cell('fxp_save_dst'))).toEqual(Array(fx.store_halfwords).fill(0));
          await mainPage(runtime);
        }
        await ui().fxPage(3);
        const rowKit = pageKit(runtime, profile, 0);
        const turned = [await rowKit.setRaw('ENC A', 0, 0x6400), await rowKit.setRaw('LEVEL', 7, 0x4000)];
        await kit.poke(32, 0x3000);
        const stockSaved = await stock();
        await mainPage(runtime);
        const saved = await values();
        expect([saved[0], saved[7]]).toEqual(turned);
        await note('turned');
        const wordsSaved = await rowWords();
        const hash = await page('fx-page4-turned');
        await mainPage(runtime);
        const firstKit = await word(session, fx.kit_ptr);
        await runtime.ui.tap('PTN', { until: { ms: 800, why: REDRAW } });
        await runtime.ui.tap('TRIG 2', { until: { ms: 300, why: REDRAW } });
        let waited = 300;
        while ((await mark()) !== MARK && waited < 8000) {
          await runtime.ui.wait({ ms: 100, why: SETTLE });
          waited += 100;
        }
        expect(await word(session, fx.kit_ptr), 'A02 owns another kit object').not.toBe(firstKit);
        expect(isDefault(await values(), defaultsOne)).toBe(true);
        expect(await mark()).toBe(MARK);
        expect((await values()).slice(0, 8)).not.toEqual(saved.slice(0, 8));
        await runtime.ui.wait({ ms: 1200, why: SETTLE });
        await runtime.ui.tap('PTN', { until: { ms: 800, why: REDRAW } });
        await runtime.ui.tap('TRIG 1', { until: { ms: 1500, why: REDRAW } });
        expect(await word(session, fx.kit_ptr), 'A01 retains its kit object').toBe(firstKit);
        expect((await values()).slice(0, 8)).toEqual(saved.slice(0, 8));
        const saves = await cell('fxp_nsave');
        await ui().saveKit(2, 'k2');
        expect(await cell('fxp_nsave')).toBeGreaterThan(saves);
        // the storage record the save hook writes (assembler, not testable on the host): the values of rows with an effect, zeros for the others, then the ids
        const source = await cell('fxp_last_save');
        const sourceValues = await Promise.all(fx.value_indices.map((index) => halfword(session, source + 2 * index)));
        expect(await spare(await cell('fxp_save_dst'))).toEqual(
          expectedRecord(effectIds, sourceValues, fx.store_halfwords),
        );
        await kit.poke(fx.value_indices[0]!, 0x1800);
        await kit.poke(fx.value_indices[7]!, 0x7f00);
        await kit.poke(32, 0x1000);
        await runtime.ui.wait({ ms: 300, why: SETTLE });
        await mainPage(runtime);
        const changed = await values();
        for (const index of [0, 7]) expect(changed[index]).not.toBe(saved[index]);
        await note('changed');
        const loads = await cell('fxp_nload');
        const loaded = await ui().loadKit(2, 'k2');
        expect(await cell('fxp_nload')).toBeGreaterThan(loads);
        expect((await values()).slice(0, 8)).toEqual(saved.slice(0, 8));
        expect(loaded.file).toEqual(expectedRecord(effectIds, saved, fx.store_halfwords));
        expect(Math.abs((await stock()) - stockSaved)).toBeLessThanOrEqual(0x40);
        expect(await rowWords()).toEqual(wordsSaved);
        noZero('kit', loaded.seen);
        await note('loaded');
        // one compare: a kit load reproduces the sound of the saved values (the gain law is on the rig), and the compare tells a changed kit from a loaded one
        await ratio('turned', 'loaded', 1, true);
        const differs = await pcmMetrics(pcm.get('changed')!, pcm.get('loaded')!, 1, artifacts, 'changed-v-loaded');
        expect(differs.identical).toBe(false);
        expect(await page('fx-page4-loaded')).toEqual(hash);
        await mainPage(runtime);
        const stockLoaded = await ui().loadKit(1, 'k1b');
        const defaults = await values();
        expect(isDefault(defaults, defaultsOne)).toBe(true);
        expect(defaults.slice(0, 8)).not.toEqual(saved.slice(0, 8));
        noZero('stock-kit', stockLoaded.seen);
        expect(stockLoaded.file).toEqual(Array(fx.store_halfwords).fill(0));
        expect(Math.abs((await stock()) - stockK1)).toBeLessThanOrEqual(0x40);
        const projectValues = new Map([
          [0, 0x5200],
          [7, 0x3000],
        ]);
        for (const [index, value] of projectValues) await kit.poke(fx.value_indices[index]!, value);
        await runtime.ui.wait({ ms: 300, why: SETTLE });
        const projectWords = await rowWords();
        const beforeSave = await cell('fxp_nsave');
        await runtime.ui.tap('SETTINGS', { until: { ms: 300, why: REDRAW }, held: 'FUNC' });
        await shot(session, artifacts, 'project-save-menu');
        await runtime.ui.tap('YES', { until: { ms: 500, why: REDRAW } });
        await runtime.ui.tap('YES', { until: { ms: 500, why: REDRAW } });
        await runtime.ui.wait({ ms: 3500, why: SETTLE });
        await shot(session, artifacts, 'project-saved');
        expect((await cell('fxp_nsave')) - beforeSave).toBeGreaterThanOrEqual(100);
        for (const [index, value] of projectValues)
          await kit.poke(fx.value_indices[index]!, 0x7f00 - Math.floor(value / 2));
        await runtime.ui.wait({ ms: 300, why: SETTLE });
        await runtime.ui.tap('SETTINGS', { until: { ms: 300, why: REDRAW } });
        for (let i = 0; i < 3; i++) await runtime.ui.tap('YES', { until: { ms: 500, why: REDRAW } });
        const projectSeen = await watchFxKitRows(runtime, plan, profile, 'YES', 7000, 10);
        await shot(session, artifacts, 'project-loaded');
        const projectGot = await values();
        expect([...projectValues.keys()].map((index) => projectGot[index])).toEqual([...projectValues.values()]);
        expect(await rowWords()).toEqual(projectWords);
        noZero('project', projectSeen);
        await context.evidence(runtime, 'persistence-complete');
      } finally {
        for (const dk of protocols) dk.cancelPending();
      }
    });
  });
}
