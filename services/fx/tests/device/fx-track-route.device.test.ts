import path from 'node:path';
import { button, encoder } from '@digi/test-support';
import { expect, test } from 'vitest';
import { addedFactorOf, compare } from '@digi/regression/runtime/pcm-files.ts';
import { bounded, rpc, word } from '@digi/regression/runtime/guest-access.ts';
import { frames } from '@digi/regression/runtime/dsp-state.ts';
import { REDRAW, SETTLE } from '@digi/regression/runtime/panel-waits.ts';
import { FX, SINE } from '@digi/regression/runtime/fixtures.ts';
import { shot, selectSine } from '@digi/regression/runtime/fx-session.ts';
import {
  amountShown,
  kitMenu,
  kitSlotOne,
  selectTrack,
  setAmount,
  soundImage,
  trackNote,
} from '@digi/regression/runtime/fx-track-ui.ts';
import { record } from '@digi/regression/runtime/artifacts.ts';
import { lockCheck } from '@digi/regression/runtime/project-kit.ts';
import { overlayWrites } from '@digi/regression/runtime/write-safety.ts';
import { names, routeKey, sendKey, trackRouteSuite } from './fx-track-route-support.ts';

// Track FX send page (setup: fx-track-route-support.ts). One session per device runs the stock, send, kit, limit and lock cases in this order
// after one base capture: the cases share the four cards (sine, master, send, limit), so a boot and a plan are paid once, not per file.

trackRouteSuite('route', { initial: 'sine', presets: ['sine', 'master', 'send', 'limit'], base: true }, (t) => {
  const { device, context, triggerFrame, base, pcm, locks, sym, chain, amounts, capacity, settings, pageTwo } = t;
  const boot = t.booter('sine');
  test('preserves stock pixels, stock PCM and master effects without a send amount', async () => {
        // stock FX pixels and PCM equal the image without the hook
        console.info('track FX leg:', 'stock FX pixels and PCM equal the image without the hook');
        {
          const { runtime, state } = await boot('stock', [SINE], 'sine');
          expect(state.results).toEqual([0]);
          // no effect is loaded, so the fx service is not on the card and the stock key list is untouched: there is no `DKRT_FXP_SEND_COUNT` to read
          await selectSine(runtime, context.profile);
          await runtime.ui.tap('FX', { until: { ms: 400, why: REDRAW } });
          expect(await shot(runtime.session, context.artifacts, 'stock-fx1')).toBe(base.hash);
          await runtime.ui.tap('NO', { until: { ms: 300, why: REDRAW } });
          const file = path.join(context.artifacts, 'stock.wav');
          await trackNote(runtime, file, triggerFrame);
          await capacity(runtime, 0);
          await compare(base.pcm, file, 1, context.artifacts, 'stock-no-hook', true);
          await context.evidence(runtime, 'stock-audible', true);
        }


        // a master effect preserves the original FX pixels and has no send amount
        console.info('track FX leg:', 'a master effect preserves the original FX pixels and has no send amount');
        {
          const { runtime, state } = await boot('master', [SINE, FX], 'master');
          expect(state.results).toEqual([0, 0]);
          // the route flags of the effect row are host-tested (services/fx/tests/builder.rs `an_effect_without_routes_runs_on_the_master_only_...`)
          expect((await chain(runtime))[0]![0]).not.toBe(0xffffffff);
          await selectSine(runtime, context.profile);
          await runtime.ui.tap('FX', { until: { ms: 400, why: REDRAW } });
          expect(await shot(runtime.session, context.artifacts, 'master-fx1')).toBe(base.hash);
          await capacity(runtime, 0);
        }

  });

  test('send knobs affect only their Sound word, track DSP amounts and settings, with squared PCM gain', async () => {
        // dry input remains available as a separate send PCM control
        console.info('track FX leg:', 'dry input remains available as a separate send PCM control');
        {
          const { runtime } = await boot('dry', [SINE], 'sine');
          await selectSine(runtime, context.profile);
          const file = path.join(context.artifacts, 'dry.wav');
          await trackNote(runtime, file, triggerFrame);
          pcm.set('dry', file);
          await context.evidence(runtime, 'dry-audible', true);
        }


        // send knobs affect only their Sound word, track DSP amounts and settings, with squared PCM gain
        console.info(
          'track FX leg:',
          'send knobs affect only their Sound word, track DSP amounts and settings, with squared PCM gain',
        );
        {
          const { runtime, state } = await boot('send', [SINE, FX], 'send');
          const { session } = runtime;
          const { profile, artifacts } = context;
          expect(state.results).toEqual([0, 0]);
          expect((await chain(runtime))[0]![0]).not.toBe(0xffffffff);
          const info = sym('DKRT_FXP_TAB');
          const registry = sym('rows_info');
          const [regRows, regSize, regFirst, templateOffset] = await Promise.all(
            [21, 22, 23, 28].map((index) => word(session, registry + index * 4)),
          );
          const code = await word(session, info + 42 * 4);
          const template = await word(session, regRows! + (code - regFirst!) * regSize! + templateOffset!);
          const classes = profile.value_classes;
          expect(template, 'send knob uses the no-lock percent template').toBe(profile.fx.template_percent);
          expect((await word(session, classes + 0x44 * template)) & 9).not.toBe(0);
          expect((await word(session, classes + 0x44 * profile.fx.send_template)) & 9, 'stock send lock control').toBe(
            0,
          );
          expect(await word(session, sym('fxp_send_row') + 8)).toBe(code);
          expect(await word(session, sym('fxp_send_count'))).toBe(2);
          await capacity(runtime, 1);
          await selectSine(runtime, profile);
          await runtime.ui.tap('FX', { until: { ms: 400, why: REDRAW } });
          const page1 = await shot(session, artifacts, 'send-page1');
          await runtime.ui.tap('FX', { until: { ms: 400, why: REDRAW } });
          const page2 = await shot(session, artifacts, 'send-page2-track1');
          expect(page1).not.toBe(base.hash);
          expect(page2).not.toBe(page1);
          await runtime.ui.tap('FX', { until: { ms: 400, why: REDRAW } });
          expect(await shot(session, artifacts, 'send-page1-again')).toBe(page1);
          await runtime.ui.tap('FX', { until: { ms: 400, why: REDRAW } });
          const before = await soundImage(session, profile, 0);
          for (const index of profile.fx.send_indices) expect(before[0x14 / 2 + index]).toBe(0);
          await setAmount(runtime, profile, 'ENC A', 0, 40);
          await shot(session, artifacts, 'send-turning');
          const after = await soundImage(session, profile, 0);
          expect(
            after.flatMap((value, index) => (value === before[index] ? [] : [index])),
            'no other Sound word changes, record slot included',
          ).toEqual([0x14 / 2 + profile.fx.send_indices[0]!]);
          await runtime.ui.wait({ ms: 300, why: SETTLE });
          expect(await amounts(runtime)).toEqual([40, ...Array(15).fill(0)]);
          await pageTwo(runtime);
          const at40 = await shot(session, artifacts, 'send-page2-track1-40');
          await selectTrack(runtime, 2);
          await shot(session, artifacts, 'send-page2-track2-after-select');
          expect(await amountShown(session, profile, 1)).toBe(0);
          await pageTwo(runtime);
          await shot(session, artifacts, 'send-page2-track2-0');
          await setAmount(runtime, profile, 'ENC A', 1, 100);
          await shot(session, artifacts, 'send-turning-track2');
          await runtime.ui.wait({ ms: 300, why: SETTLE });
          expect((await amounts(runtime)).slice(0, 2)).toEqual([40, 100]);
          await selectTrack(runtime, 1);
          await pageTwo(runtime);
          const back = await shot(session, artifacts, 'send-page2-track1-back');
          expect(back === at40 || (await amountShown(session, profile, 0)) === 40).toBe(true);
          await runtime.ui.wait({ ms: 2600, why: SETTLE });
          const stored = await settings(runtime, 'send-after');
          expect(stored.seq).toBeGreaterThan(1);
          expect(stored.entries.get(sendKey)).toBe('2864' + '00'.repeat(14));
          expect(stored.entries.get(routeKey(FX))).toBe('01');
          const writes = overlayWrites(stored.overlay, device);
          record(artifacts, 'send-store-writes', writes);
          const allowed = [
            profile.settings.copy_a,
            profile.settings.copy_b,
            profile.selection.copy_a,
            profile.selection.copy_b,
          ];
          expect(writes.store_sectors.every((sector) => allowed.includes(sector))).toBe(true);
          expect(
            writes.store_sectors.some(
              (sector) => sector === profile.settings.copy_a || sector === profile.settings.copy_b,
            ),
          ).toBe(true);
          await runtime.ui.tap('NO', { until: { ms: 300, why: REDRAW } });
          const file = path.join(artifacts, 'send.wav');
          await trackNote(runtime, file, (await frames(session)) + 300);
          pcm.set('send', file);
          const result = { added_factor: addedFactorOf(pcm.get('dry')!, file) };
          record(artifacts, 'send-added-factor', result);
          const want = 0.5 * (40 / 127) ** 2;
          expect(Math.abs(result.added_factor / want - 1)).toBeLessThan(0.02);
          await context.evidence(runtime, 'send-audible', true);
        }

  });

  test('kit loading never zeros amounts and another pattern receives the same settings', async () => {
        // kit loading never zeros amounts and another pattern receives the same settings
        console.info('track FX leg:', 'kit loading never zeros amounts and another pattern receives the same settings');
        {
          const { runtime } = await boot('kit', [SINE, FX], 'send');
          const { session } = runtime;
          const { profile, artifacts } = context;
          await selectSine(runtime, profile);
          await pageTwo(runtime);
          await setAmount(runtime, profile, 'ENC A', 0, 40);
          await kitMenu(runtime, 1);
          await runtime.ui.tap('YES', { until: { ms: 600, why: REDRAW } });
          await kitSlotOne(runtime);
          await shot(session, artifacts, 'kit-save-slot');
          await runtime.ui.tap('YES', { until: { ms: 600, why: REDRAW } });
          await runtime.ui.tap('YES', { until: { ms: 1500, why: REDRAW } });
          await shot(session, artifacts, 'kit-saved');
          await runtime.ui.wait({ ms: 500, why: SETTLE });
          await selectTrack(runtime, 2);
          await pageTwo(runtime);
          await setAmount(runtime, profile, 'ENC A', 1, 100);
          await runtime.ui.wait({ ms: 2600, why: SETTLE });
          const nload = await word(session, sym('fxp_nload'));
          await kitMenu(runtime, 0);
          await runtime.ui.tap('YES', { until: { ms: 700, why: REDRAW } });
          await kitSlotOne(runtime);
          const seen: [number, number][] = [];
          try {
            await bounded(() => session.client.inputButton(button('YES'), true));
            for (let ms = 0; ms < 700; ms += 2) {
              await runtime.ui.wait({ ms: 2, why: SETTLE });
              const value = (await chain(runtime))[0]![19]!;
              if (!seen.length || seen.at(-1)![1] !== value) seen.push([ms, value]);
              if (ms === 60) await bounded(() => session.client.inputButton(button('YES'), false));
            }
          } finally {
            await bounded(() => session.client.inputButton(button('YES'), false));
          }
          await runtime.ui.wait({ ms: 500, why: SETTLE });
          expect(await word(session, sym('fxp_nload'))).not.toBe(nload);
          expect(await word(session, sym('DKRT_FXS') + 16)).toBeGreaterThan(0);
          expect(await amountShown(session, profile, 0)).toBe(40);
          expect(await amountShown(session, profile, 1)).toBe(100);
          expect(seen.length).toBeGreaterThan(0);
          expect(seen.every(([, value]) => value === seen[0]![1])).toBe(true);
          expect(seen[0]![1] & 0xffff).toBe((100 << 8) | 40);
          record(artifacts, 'kit-2ms-amount-observations', seen);
          const firstKit = await word(session, profile.fx.kit_ptr);
          await runtime.ui.tap('PTN', { until: { ms: 800, why: REDRAW } });
          await runtime.ui.tap('TRIG 2', { until: { ms: 300, why: REDRAW } });
          await runtime.ui.wait({ ms: 1500, why: SETTLE });
          expect(await word(session, profile.fx.kit_ptr), 'A02 owns another kit object').not.toBe(firstKit);
          expect(await amountShown(session, profile, 0)).toBe(40);
          expect(await amountShown(session, profile, 1)).toBe(100);
          expect((await amounts(runtime)).slice(0, 2)).toEqual([40, 100]);
          await runtime.ui.tap('PTN', { until: { ms: 800, why: REDRAW } });
          await runtime.ui.tap('TRIG 1', { until: { ms: 1500, why: REDRAW } });
          expect(await amountShown(session, profile, 0)).toBe(40);
          expect(await amountShown(session, profile, 1)).toBe(100);
          await runtime.ui.wait({ ms: 2600, why: SETTLE });
          expect((await settings(runtime, 'kit-after')).entries.get(sendKey)?.slice(0, 4)).toBe('2864');
        }

  });

  test('the device send limit leaves excess effects on the master without send cells', async () => {
        // the device send limit leaves excess effects on the master without send cells
        console.info('track FX leg:', 'the device send limit leaves excess effects on the master without send cells');
        {
          const { runtime, state } = await boot('limit', [SINE, ...names], 'limit');
          expect(state.results).toEqual([0, 0, 0, 0]);
          // which effects get a send cell is host-tested (services/fx/tests/table.rs `the_send_limit_is_the_cells_of_the_table_...`); the device shows capacity() and the amounts below
          await capacity(runtime, context.sendSlots);
          await selectSine(runtime, context.profile);
          await pageTwo(runtime);
          await shot(runtime.session, context.artifacts, 'limit-page2');
          await setAmount(runtime, context.profile, 'ENC A', 0, 20);
          if (context.sendSlots >= 2) await setAmount(runtime, context.profile, 'ENC B', 0, 30, 1);
          await runtime.ui.wait({ ms: 300, why: SETTLE });
          expect((await amounts(runtime, 0))[0]).toBe(20);
          expect((await amounts(runtime, 1))[0]).toBe(context.sendSlots >= 2 ? 30 : 0);
          expect((await amounts(runtime, 2))[0]).toBe(0);
        }

  });

  test('project save controls stay ordered: base, none, amount, send', async () => {
        for (const turn of ['base', 'none', 'amount', 'send'] as const) {
          // Project save controls stay ordered: base, none, amount, send.
          console.info('track FX leg:', `project save control: ${turn}`);
          {
            const { runtime } = await boot(`lock-${turn}`, [SINE, FX], 'send');
            const { session } = runtime;
            const { artifacts, profile } = context;
            await selectSine(runtime, profile);
            await runtime.ui.tap('NO', { until: { ms: 300, why: REDRAW } });
            if (turn === 'send' || turn === 'amount') await pageTwo(runtime);
            await shot(session, artifacts, `lock-${turn}-page`);
            const held = turn === 'none' || turn === 'send';
            let cc: { hex: string };
            try {
              if (held) {
                await runtime.ui.tap('RECORD', { until: { ms: 200, why: REDRAW } });
                await runtime.ui.tap('TRIG 1', { until: { ms: 200, why: REDRAW } });
                await bounded(() => session.client.inputButton(button('TRIG 1'), true));
                await runtime.ui.wait({ ms: 100, why: SETTLE });
              }
              await rpc(session, 'midi.out');
              if (turn === 'amount') await setAmount(runtime, profile, 'ENC A', 0, 40);
              else if (turn === 'send') {
                await bounded(() => session.client.inputTurn(encoder('ENC A'), 11));
                await runtime.ui.wait({ ms: 10, why: SETTLE });
              }
              await runtime.ui.wait({ ms: 300, why: SETTLE });
              await shot(session, artifacts, `lock-${turn}-turned`);
              cc = await rpc<{ hex: string }>(session, 'midi.out');
            } finally {
              if (held) await bounded(() => session.client.inputButton(button('TRIG 1'), false));
            }
            if (held) {
              await runtime.ui.wait({ ms: 300, why: SETTLE });
              await runtime.ui.tap('RECORD', { until: { ms: 300, why: REDRAW } });
            }
            const shown = await amountShown(session, profile, 0);
            await runtime.ui.tap('SETTINGS', { until: { ms: 400, why: REDRAW }, held: 'FUNC' });
            await runtime.ui.tap('YES', { until: { ms: 700, why: REDRAW } });
            await runtime.ui.tap('YES', { until: { ms: 500, why: REDRAW } });
            await runtime.ui.wait({ ms: 3500, why: SETTLE });
            const overlay = path.join(artifacts, `lock-${turn}.overlay.bin`);
            await rpc(session, 'card.overlay', { path: overlay });
            locks.set(turn, { overlay, cc: cc!.hex, shown });
            record(artifacts, `lock-${turn}-result`, { overlay, cc: cc!.hex, shown });
            if (turn === 'send') {
              const result = lockCheck(device, context.baseline('send').cardImage!, {
                base: locks.get('base')!.overlay,
                none: locks.get('none')!.overlay,
                amount: locks.get('amount')!.overlay,
                send: locks.get('send')!.overlay,
              });
              record(artifacts, 'lock-project-byte-oracles', result);
              expect(result.trig_changed_bytes, 'panel script records a trig: method control').toBeGreaterThan(0);
              // The legacy script could not produce a stock-knob lock control. Preserve
              // its bounded claim: same pattern bytes, supported by the class flag above.
              expect(result.send_changed_bytes, 'held send turn saves no lock bytes').toBe(0);
              expect(locks.get('amount')!.shown).toBe(40);
              expect(
                (result.amount_diff.differBytes.kit ?? 0) + (result.amount_diff.differBytes.pool ?? 0),
                'amount saves no kit or Sound record bytes',
              ).toBe(0);
              expect(locks.get('send')!.cc, 'send knob emits no MIDI CC').toBe('');
            }
          }
        }
  });
});
