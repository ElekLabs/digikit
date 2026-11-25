import path from 'node:path';
import { button, stepWithProgress } from '@digi/test-support';
import { describe, expect, test } from 'vitest';
import { withWaitApi, type TestRuntime } from '@digi/regression/support.ts';
import { compare } from '@digi/regression/runtime/pcm-files.ts';
import { bounded, rpc, word } from '@digi/regression/runtime/guest-access.ts';
import { frames, runToFrame } from '@digi/regression/runtime/dsp-state.ts';
import { pickSine, REDRAW, SETTLE } from '@digi/regression/runtime/panel-waits.ts';
import { SEND, SINE, triggerFrame } from '@digi/regression/runtime/fixtures.ts';
import { withRouteFxSession } from '@digi/regression/runtime/fx-route-live.ts';
import { inspectFxBoot, withPausedFxProtocol } from '@digi/regression/runtime/fx-session.ts';
import { listLine, MASTER, routingUi, SEND_ROUTE } from '@digi/regression/runtime/fx-routing-ui.ts';
import { record } from '@digi/regression/runtime/artifacts.ts';
import { patchkit } from '@digi/regression/runtime/patchkit.ts';
import { DEVICES, FIRMWARE } from '@digi/regression/runtime/devices.ts';
import { expectRuntimeUi } from '@digi/regression/runtime/ready.ts';
import { emulatorDk } from '@digi/regression/runtime/waits.ts';

const routeKey = `fx/route/${SEND}`;

async function compareRoutePcm(base: string, sample: string, gain: number, artifacts: string, tag: string) {
  // Separate boots may move the onset by up to four 64-sample DSP sub-blocks (128 frames).
  await compare(base, sample, gain, artifacts, tag, gain === 1);
}

/** Native store reader observes the card after guest writes; no settings encoding in this test. */
async function storedRoute(runtime: TestRuntime, artifacts: string, tag: string) {
  const base = runtime.session.cardPath;
  if (!base) throw new Error('route persistence needs the loaded base card');
  const overlay = path.join(artifacts, `${tag}.overlay.bin`);
  await rpc(runtime.session, 'card.overlay', { path: overlay });
  const dumped = await patchkit(
    ['store', 'dump', base, '--device', runtime.session.options.device, '--overlay', overlay, '--json'],
    { timeout: 30_000 },
  );
  const dump = JSON.parse(dumped.stdout) as {
    settings: { active: 'A' | 'B' | null; copies: { entries: { key: string; value: string }[] }[] };
  };
  record(artifacts, `${tag}-store`, dump);
  expect(dump.settings.active).not.toBeNull();
  return dump.settings.copies[dump.settings.active === 'A' ? 0 : 1]!.entries.find((entry) => entry.key === routeKey)
    ?.value;
}

for (const device of DEVICES) {
  describe(`${device.toUpperCase()} ${FIRMWARE[device]} FX route persistence`, () => {
    const context = withRouteFxSession(device, 'FX routes dry/apply/back', {
      image: 'routes',
      initial: 'dry',
      presets: {
        dry: { selection: ['modules/m0.dkm'] },
        apply: { selection: ['modules/m0.dkm', 'modules/m2.dkm'] },
        back: { selection: ['modules/m0.dkm', 'modules/m2.dkm'], settings: [[routeKey, '00']] },
      },
      requireAudible: true,
    });

    test('a pick in FX ROUTING persists both route directions and a restart boots into the matching audio and FX pages', async () => {
      const { fixture, plan, artifacts, slots, rememberEpoch } = context;
      const inspect = async (runtime: TestRuntime, tag: string, effect: boolean, afterEpoch?: number) => {
        const state = await inspectFxBoot(runtime, plan, effect ? [SINE, SEND] : [SINE], artifacts, tag, undefined, {
          afterEpoch,
        });
        rememberEpoch(state.epoch);
        return state;
      };
      const capture = async (runtime: TestRuntime, tag: string) => {
        const { session } = runtime;
        await rpc(session, 'run.mode', { mode: 'paused' });
        await rpc(session, 'pcm.tap', { action: 'start' });
        const hint = device === 'dt2' ? 7 : 5;
        const sine = (await withPausedFxProtocol(runtime, (dk) => dk.modules())).find((module) => module.name === SINE);
        expect(sine?.types).toHaveLength(1);
        if (device === 'dt2') {
          let selected = -1;
          for (const position of [hint, hint + 1, hint - 1, hint + 2, hint + 3]) {
            await pickSine(runtime, position);
            // Legacy DT2 selected-track type observation (finding 02).
            selected = ((await word(session, context.profile.track_type_word)) >>> 8) & 0xff;
            if (selected === sine!.types[0]) break;
          }
          expect(selected, 'the selected DT2 machine is the runtime sine').toBe(sine!.types[0]);
        } else await pickSine(runtime, hint);
        await runtime.ui.tap('NO', { until: { ms: 300, why: REDRAW } });
        const current = await frames(session);
        const trigger = Math.max(triggerFrame(device), current + 300);
        record(artifacts, `${tag}-timing`, { currentFrame: current, triggerFrame: trigger });
        const file = path.join(artifacts, `${tag}.wav`);
        try {
          await runToFrame(session, trigger);
          // 44,000 ColdFire instructions (half a DSP block of 88,000), not frames: places the TRIG press inside the block so that separate boots compare onsets; no firmware state replaces it
          await stepWithProgress(session.client, 44_000);
          await rpc(session, 'pcm.tap', { action: 'capture', path: file });
          await bounded(() => session.client.inputButton(button('TRIG 1'), true));
          await runToFrame(session, trigger + 1500);
        } finally {
          try {
            await rpc(session, 'pcm.tap', { action: 'capture_stop' });
          } finally {
            await bounded(() => session.client.inputButton(button('TRIG 1'), false));
          }
        }
        await runtime.ui.wait({ ms: 50, why: SETTLE });
        await bounded(() => session.expectNoFault(`FX routes ${tag}`));
        return file;
      };
      const expectRoute = async (runtime: TestRuntime, send: boolean) => {
        const ui = routingUi(runtime, plan, artifacts, slots);
        expect(await ui.routes(1)).toEqual([send ? 1 : 0]);
        expect(await ui.countCell()).toBe(send ? 2 : 0);
        expect((await ui.sendUsed())!.used).toBe(send ? 1 : 0);
      };
      const pages = async (runtime: TestRuntime, tag: string, send: boolean) => {
        const ui = routingUi(runtime, plan, artifacts, slots);
        await runtime.ui.tap('FX', { until: { ms: 400, why: REDRAW } });
        const first = await ui.shot(`${tag}-fx1`);
        await runtime.ui.tap('FX', { until: { ms: 400, why: REDRAW } });
        const second = await ui.shot(`${tag}-fx2`);
        if (send) expect(second).not.toBe(first);
      };
      const apply = async (runtime: TestRuntime, tag: 'apply' | 'back', send: boolean) => {
        const { session } = runtime;
        const ui = routingUi(runtime, plan, artifacts, slots);
        // the pick: the effect leaves the route it runs on for the other one, at once (a reload of its module, no restart)
        const list = await ui.open();
        expect(list.items).toEqual([listLine(SEND, send ? MASTER : SEND_ROUTE)]);
        await ui.shot(`${tag}-list`);
        expect((await ui.pickEffect(0, SEND)).items).toEqual(['SEND', 'MASTER']);
        await ui.shot(`${tag}-choice`);
        const before = await ui.epoch();
        await ui.pickRoute(send ? 0 : 1);
        await ui.waitRoutes(1, [send ? SEND_ROUTE : MASTER]);
        expect(await ui.job()).toMatchObject({ phase: 2, result: 0, op: 3 });
        expect(await ui.epoch(), 'the pick restarts nothing').toBe(before);
        await ui.leave();
        await ui.shot(`${tag}-picked`);
        await context.evidence(runtime, `${tag}-before-restart`, tag === 'apply');
        rememberEpoch(before);
        session.client.drainEvents();
        await rpc(session, 'run.mode', { mode: 'free' });
        await bounded(() => runtime.dk.restart());
        runtime.dk.cancelPending();
        const dk = emulatorDk(session);
        const reboot = withWaitApi({ session, dk, close: runtime.close });
        try {
          await expectRuntimeUi(session, plan, artifacts, { afterEpoch: before });
          await bounded(() => session.client.guardSet(context.baseline(tag).guard));
          await inspect(reboot, `${tag}-restarted`, true, before);
          await expectRoute(reboot, send);
          expect(await storedRoute(reboot, artifacts, tag)).toBe(send ? 'send' : 'master');
          const file = await capture(reboot, tag === 'apply' ? 'master' : 'send1');
          await pages(reboot, `${tag}-after`, send);
          await context.evidence(reboot, `${tag}-after-restart`);
          // the list shows the route the settings gave the restarted boot
          const afterUi = routingUi(reboot, plan, artifacts, slots);
          expect((await afterUi.open()).items).toEqual([listLine(SEND, send ? SEND_ROUTE : MASTER)]);
          await afterUi.leave();
          return file;
        } finally {
          dk.cancelPending();
        }
      };

      const dryRuntime = fixture.runtime;
      await inspect(dryRuntime, 'dry-boot', false);
      const dry = await capture(dryRuntime, 'dry');
      await context.evidence(dryRuntime, 'dry');
      const sendRuntime = await fixture.reload(context.baseline('apply'));
      await inspect(sendRuntime, 'apply-boot', true);
      await expectRoute(sendRuntime, true);
      const send0 = await capture(sendRuntime, 'send0');
      await pages(sendRuntime, 'apply-before', true);
      const master = await apply(sendRuntime, 'apply', false);
      // Independent clean baseline: two quick retained restarts would trigger unstable-boot safe mode.
      const masterRuntime = await fixture.reload(context.baseline('back'));
      await inspect(masterRuntime, 'back-boot', true);
      await expectRoute(masterRuntime, false);
      const send1 = await apply(masterRuntime, 'back', true);
      await compareRoutePcm(dry, send0, 1, artifacts, 'send0');
      await compareRoutePcm(dry, master, 0.5, artifacts, 'master');
      await compareRoutePcm(dry, send1, 1, artifacts, 'send1');
    });
  });
}
