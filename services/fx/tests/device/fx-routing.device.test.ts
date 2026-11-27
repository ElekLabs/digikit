import { describe, expect, test } from 'vitest';
import { deviceProfile } from '@digi/regression/runtime/device-data.ts';
import { record } from '@digi/regression/runtime/artifacts.ts';
import { DEVICES, FIRMWARE } from '@digi/regression/runtime/devices.ts';
import { GAIN, SEND, SINE } from '@digi/regression/runtime/fixtures.ts';
import { SEND3, SENDONLY, withRouteFxSession } from '@digi/regression/runtime/fx-route-live.ts';
import { inspectFxBoot } from '@digi/regression/runtime/fx-session.ts';
import { listLine, MASTER, routingUi, SEND_ROUTE } from '@digi/regression/runtime/fx-routing-ui.ts';

/**
 * FX ROUTING in the SETTINGS list on the firmware's own views (docs/findings/65-fx-routes.md). What only the firmware shows: the stock SETTINGS list holds the row, the stock choice popup
 * gives the picked index back to the service (twice: the effect, then the route), a route pick moves the effect's DSP row to the send bus without a restart, and a full send bus
 * takes SEND out of the popup. The popups are read from the firmware's window objects. The rules (which routes are offered, the toasts, a refused or deferred reload, the
 * saved key) are host tests of `services/fx` (`routing/tests.rs`, `fxsend/tests.rs`).
 */
const selection = (...indices: number[]) => indices.map((index) => `modules/m${index}.dkm`);

for (const device of DEVICES) {
  const sendCells = deviceProfile(device).limits.send_slots!;
  describe(`${device.toUpperCase()} ${FIRMWARE[device]} FX routing`, () => {
    const context = withRouteFxSession(device, 'FX routing', {
      image: 'routes',
      initial: 'list',
      requireAudible: false,
      presets: {
        // the send effect starts on the master route, so that a pick moves it to the send bus
        list: { selection: selection(0, 1, 2), settings: [[`fx/route/${SEND}`, '00']] },
        // the send cells are taken: DT2 holds two (SEND and SEND-ONLY), DN2 one (SEND); SEND-3 is on the master route
        full: {
          selection: sendCells > 1 ? selection(2, 3, 4) : selection(2, 4),
          settings: [[`fx/route/${SEND3}`, '00']],
        },
      },
    });

    test('the SETTINGS row opens the effects, a pick moves an effect to the send bus without a restart, and a full bus offers no SEND', async () => {
      const { fixture, plan, artifacts, slots, sendSlots } = context;
      let runtime = fixture.runtime;
      const inspect = async (tag: string, names: string[]) => {
        await inspectFxBoot(runtime, plan, names, artifacts, tag);
        context.rememberEpoch(runtime.epoch);
        return routingUi(runtime, plan, artifacts, slots);
      };

      let ui = await inspect('list', [SINE, GAIN, SEND]);
      expect(await ui.routes(2), 'both effects start on the master route').toEqual([MASTER, MASTER]);
      expect(await ui.sendUsed()).toEqual({ limit: sendSlots, used: 0, largest: 0 });
      const epoch = await ui.epoch();

      // the stock SETTINGS list ends in the row: its action opens the popup that the service builds
      const list = await ui.open();
      expect(list.items, 'one row for each effect with a page').toEqual([listLine(GAIN, MASTER), listLine(SEND, MASTER)]);
      const png = await ui.png('fx-routing-list');
      record(artifacts, 'fx-routing-screenshot', { png });

      // the stock choice returns the picked index: row 1 opens the popup of the second effect, and its route index 0 is SEND
      const routes = await ui.pickEffect(1, SEND);
      expect(routes.items, 'the effect may take both routes').toEqual(['SEND', 'MASTER']);
      await ui.pickRoute(0);
      await ui.waitRoutes(2, [MASTER, SEND_ROUTE]);
      expect(await ui.sendUsed(), 'the send cell is in use').toEqual({ limit: sendSlots, used: 1, largest: 0 });
      expect(await ui.countCell(), 'the send pages are listed').toBe(2);
      expect(await ui.job(), 'the reload job of the module ended').toMatchObject({ phase: 2, result: 0, op: 3 });
      expect(await ui.epoch(), 'no restart').toBe(epoch);
      await ui.leave();

      // and back: the list shows the route in force, the second popup offers the saved send first
      const again = await ui.open();
      expect(again.items).toEqual([listLine(GAIN, MASTER), listLine(SEND, SEND_ROUTE)]);
      await ui.pickEffect(1, SEND);
      await ui.pickRoute(1);
      await ui.waitRoutes(2, [MASTER, MASTER]);
      expect((await ui.sendUsed())!.used).toBe(0);
      expect(await ui.epoch(), 'no restart').toBe(epoch);
      await ui.leave();
      await runtime.session.expectNoFault('FX routing picks');

      // a full send bus: SEND is not offered to the effect that is on the master route, and is again once another effect leaves the bus
      const previous = runtime;
      runtime = await fixture.reload(context.baseline('full'));
      expect(runtime.session).toBe(previous.session);
      const fullIds = sendSlots > 1 ? [SEND, SENDONLY, SEND3] : [SEND, SEND3];
      ui = await inspect('full', fullIds);
      const fullRoutes = [...Array<number>(sendSlots).fill(SEND_ROUTE), MASTER];
      expect(await ui.routes(fullIds.length)).toEqual(fullRoutes);
      expect(await ui.sendUsed()).toEqual({ limit: sendSlots, used: sendSlots, largest: 0 });
      const full = await ui.open();
      expect(full.items).toEqual(fullIds.map((id, i) => listLine(id, fullRoutes[i]!)));
      await ui.shot('fx-routing-full');
      const last = fullIds.length - 1;
      expect((await ui.pickEffect(last, SEND3)).items, 'no free send cell: SEND is not offered').toEqual(['MASTER']);
      await ui.pickRoute(0);
      await ui.leave();
      expect(await ui.routes(fullIds.length), 'the same route changes nothing').toEqual(fullRoutes);

      await ui.open();
      await ui.pickEffect(0, SEND);
      await ui.pickRoute(1);
      await ui.waitRoutes(fullIds.length, [MASTER, ...fullRoutes.slice(1)]);
      await ui.leave();
      await ui.open();
      expect((await ui.pickEffect(last, SEND3)).items, 'a send cell is free again').toEqual(['SEND', 'MASTER']);
      await ui.pickRoute(0);
      await ui.waitRoutes(fullIds.length, [MASTER, ...fullRoutes.slice(1, -1), SEND_ROUTE]);
      await ui.leave();
      await runtime.session.expectNoFault('FX routing with a full send bus');
    });
  });
}
