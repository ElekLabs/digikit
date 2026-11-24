import {
  emulatorRead,
  readTopMenuItems,
  readViewStack,
  repoRoot,
  viewStackFacts,
  type Device,
} from '@digi/test-support';
import { plainItem, readChoiceWindow, type ChoiceWindow } from './choice-window.ts';
import { REDRAW, rows, word, type Rt } from './fx-live.ts';
import { shot, withPausedFxProtocol } from './fx-session.ts';
import { type RuntimePlan } from './runtime-plan.ts';
import { symbolsOf } from './service-symbols.ts';
import fs from 'node:fs';
import path from 'node:path';

export const MASTER = 0;
export const SEND_ROUTE = 1;

/** The text of the FX ROUTING list line for the module `id` on `route`: the part after the last `/`, upper case, cut in the middle over 19 characters, then `: SEND` or `: MASTER`. The popup pads the names to a column (`plainItem`). */
export const listLine = (id: string, route: number): string =>
  `${fitName(id.split('/').at(-1)!.toUpperCase(), LIST_NAME_MAX)}: ${route === SEND_ROUTE ? 'SEND' : 'MASTER'}`;
/** `LIST_NAME_MAX` of services/fx/src/routing.rs. */
const LIST_NAME_MAX = 19;
/** `Line::put_fit` of that file: a name over `room` keeps its start and end around `..`. */
const fitName = (name: string, room: number): string => {
  if (name.length <= room) return name;
  const keep = room - 2;
  return `${name.slice(0, keep - Math.floor(keep / 2))}..${name.slice(name.length - Math.floor(keep / 2))}`;
};
/** Is `title` the title of the route popup of `id`? The popup cuts a long name in the middle (`put_fit` of services/fx/src/routing.rs), so this checks the start and the end. */
export const isRouteTitle = (title: string, id: string): boolean =>
  title.endsWith(' ROUTE') && title.startsWith(id.split('/').at(-1)!.toUpperCase().slice(0, 4));

/**
 * FX ROUTING of the SETTINGS list on the firmware's own views: the row (the last of the stock list), the choice popup of the effects, the choice popup of the routes, and the
 * state a pick changes (the effect's DSP row route word, the send cells in use). The popups are read from the firmware's window object (`readChoiceWindow`).
 */
export function routingUi(runtime: Rt, plan: RuntimePlan, artifacts: string, slots: number) {
  const { session } = runtime;
  const device = session.options.device as Device;
  const facts = viewStackFacts(device);
  if (!facts) throw new Error(`${device} has no [ui] facts`);
  const read = emulatorRead(session);
  const depth = async () => (await readViewStack(read, facts, 32)).length;
  const sym = (name: string) => symbolsOf(runtime, plan)(name);
  let mainDepth = 0;
  /** Wait for the choice popup whose title `isTitle` accepts; its items are in `plainItem` form. A popup that cannot be read throws at once. */
  const waitChoice = async (isTitle: (title: string) => boolean, what: string): Promise<ChoiceWindow> => {
    let seen: ChoiceWindow | undefined;
    await runtime.ui.wait({
      state: async () => (seen = await readChoiceWindow(session)),
      done: (choice?: ChoiceWindow) => choice !== undefined && isTitle(choice.title),
      withinMs: 10_000,
      what: `the choice popup ${what}`,
    });
    return { ...seen!, items: seen!.items.map(plainItem) };
  };
  return {
    /** From the main screen: SETTINGS, its last row (the FX ROUTING row), YES. The effects popup. */
    open: async () => {
      // a page of the main screen (an FX page) is left first
      await runtime.ui.tap('NO', { until: { ms: 400, why: REDRAW } });
      mainDepth = await depth();
      await runtime.ui.tap('SETTINGS', {
        until: {
          state: depth,
          done: (n: number) => n > mainDepth,
          withinMs: 10_000,
          what: 'SETTINGS on the view stack',
        },
      });
      const count = await readTopMenuItems(read, facts);
      for (let i = 0; i < count; i++) await runtime.ui.tap('UP', { until: { drained: true } });
      for (let i = 0; i < count - 1; i++) await runtime.ui.tap('DOWN', { until: { drained: true } });
      await runtime.ui.tap('YES', { until: { drained: true } });
      return waitChoice((title) => title === 'FX ROUTING', 'FX ROUTING');
    },
    /** DOWN to `index`, YES: the firmware passes the index to the service, which opens the route popup of that effect. */
    pickEffect: async (index: number, id: string) => {
      for (let i = 0; i < index; i++) await runtime.ui.tap('DOWN', { until: { drained: true } });
      await runtime.ui.tap('YES', { until: { drained: true } });
      return waitChoice((title) => isRouteTitle(title, id), `ROUTE of ${id}`);
    },
    /** DOWN to `index`, YES: the route is chosen and the popup closes. */
    pickRoute: async (index: number) => {
      for (let i = 0; i < index; i++) await runtime.ui.tap('DOWN', { until: { drained: true } });
      await runtime.ui.tap('YES', { until: { drained: true } });
    },
    /** NO out of the popup and the SETTINGS list to the main screen. */
    leave: async () => {
      for (let i = 0; i < 3 && (await depth()) > mainDepth; i++)
        await runtime.ui.tap('NO', { until: { drained: true } });
      await runtime.ui.wait({
        state: depth,
        done: (n: number) => n <= mainDepth,
        withinMs: 10_000,
        what: 'back on the main screen',
      });
    },
    waitChoice,
    /** The route word of the first `count` DSP rows (0 master, 1 send). */
    routes: async (count: number) => (await rows(session, plan, slots)).slice(0, count).map((row) => row[18]!),
    /** Wait until the first `count` rows run on `want`. */
    waitRoutes: (count: number, want: number[]) =>
      runtime.ui.wait({
        state: async () => (await rows(session, plan, slots)).slice(0, count).map((row) => row[18]!),
        done: (now: number[]) => JSON.stringify(now) === JSON.stringify(want),
        withinMs: 30_000,
        what: `the DSP rows run on routes ${want}`,
      }),
    countCell: () => word(session, sym('fxp_send_count')),
    sendUsed: async () => (await withPausedFxProtocol(runtime, (dk) => dk.capacity())).SEND_SLOTS,
    /** Wait until the live job of the last pick ended, and give its state (`result` 0 is ok; `op` 3 is a reload). */
    job: async () => {
      let last: { phase: number; result: number; step: number; unloaded: boolean; op: number } | undefined;
      await runtime.ui.wait({
        state: async () => (last = await withPausedFxProtocol(runtime, (dk) => dk.reloadState())),
        done: (job?: { phase: number }) => job?.phase === 2,
        withinMs: 30_000,
        what: 'the reload job of the pick ended',
      });
      return last!;
    },
    epoch: async () => (await session.status()).epoch as number,
    shot: (tag: string) => shot(session, artifacts, tag),
    /** The current OLED as a PNG in `out/runs/fx-routing/<device>/` (the run's artifacts are removed when the test passes); the path. */
    png: async (tag: string) => {
      const dir = path.join(repoRoot(), 'out', 'runs', 'fx-routing', device);
      fs.mkdirSync(dir, { recursive: true });
      const file = path.join(dir, `${tag}.png`);
      fs.writeFileSync(file, await session.framePng());
      return file;
    },
  };
}
