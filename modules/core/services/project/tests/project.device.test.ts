import { expect, test } from 'vitest';
import { pollUntil } from '@digi/test-support';
import { currentRecord } from '@digi/regression/runtime/card-kit.ts';
import { CARD_SYMBOLS, cardRigs, RT, type CardRig } from '@digi/regression/runtime/card-rig.ts';
import {
  clearProject,
  copyPasteProject,
  dkpOf,
  E_SET_NOKEY,
  idHash,
  loadProject,
  ProjectLog,
  projectPaths,
  readStoreFile,
  renameProject,
  saveProjectAs,
  savePanelProject,
  slotFile,
  unloadLive,
  type ProtocolRun,
} from '@digi/regression/runtime/project-kit.ts';
import { word } from '@digi/regression/runtime/fx-live.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';
import { liveArtifacts } from '@digi/regression/runtime/artifacts.ts';
import { shot } from '@digi/regression/runtime/fx-session.ts';
import type { TestRuntime } from '@digi/regression/support.ts';

/**
 * The `project` service on a card (docs/design/digikit-project.md): the file of a project follows the stock project menus. `test/project-user` keeps one setting that the test
 * puts and reads by service commands; the project's own value is then the one in the file. One session per device runs the cases in order (the state carries on). The
 * cases wait on the service's events (PROJECT_LOADED, PROJECT_SAVED, PROJECT_MODULE) and on the store, not on fixed times.
 *
 * The panel flows (`savePanelProject`, `saveProjectAs`, `loadProject`, `renameProject`, `copyPasteProject`, `clearProject` of project-kit.ts) follow the stock menus as seen on
 * screen; `from` is the current project's slot, where the lists open.
 */

const USER = 'test/project-user';
const SINE = 'test/sine';
const DEBUG = 'digikit/debug';
/** The slots of the cases (the firmware's numbers, as in the events; the list shows slot 0 as 001): the first project, the second project, the pasted copy. */
const SLOT_FIRST = 0;
const SLOT_SECOND = 1;
const SLOT_COPY = 2;
/** The values the cases put under the module's key, as 32-bit words. */
const VALUE_A = 0xa1;
const VALUE_B = 0xb2;
/** The service commands of the fixture: put the body under the key, and read what the module's `init` read (see project-user.c). */
const CMD_PUT = 0x38;
const CMD_READ = 0x39;
/** The stable period of the card's selection record (DSP blocks): short, so a restart ends it within the test. */
const STABLE_BLOCKS = 300;
/** Emulated-time bound of a wait on the store or on a module's reload. */
const WITHIN_MS = 30_000;
/** The folder of the archive. */
const ARCHIVE = 'projects/archive/';

const word32 = (v: number) => {
  const b = Buffer.alloc(4);
  b.writeUInt32BE(v);
  return b;
};
const nameOfFile = (path: string) => /^projects\/\d{3}-(.*)\.dkp$/.exec(path)![1]!;
const hex8 = (v: number) => (v >>> 0).toString(16).padStart(8, '0');

cardRigs(
  'project service',
  {
    dsp: 'sharc',
    roots: RT,
    symbols: CARD_SYMBOLS,
    initial: () => ({}),
    project: true,
    uploads: true,
    relaxFs: true,
    // The stock rename of the current project holds the main loop about 1.05 s (seen on the emulator before any digikit code runs: the service has no rename hook);
    // the watchdog's 1 s default would report that stock pause as a stall. Longer pauses still fail.
    stallMs: 2000,
  },
  (run: CardRig) => {
    const spec = () => ({
      modules: [DEBUG, USER, SINE],
      selection: [DEBUG, USER, SINE].map((id) => run.mod(id).name),
      stableBlocks: STABLE_BLOCKS,
    });
    const usbOf =
      (rt: TestRuntime): ProtocolRun =>
      (f) =>
        run.usb(rt, f);

    /** What the fixture module reports, or null while it is not loaded (the request is refused then). */
    let lastRefusal = '';
    async function userState(rt: TestRuntime) {
      try {
        const r = await run.usb(rt, (dk) => dk.request(CMD_READ));
        if (r.status !== 0) {
          lastRefusal = `status ${r.status}`;
          return null;
        }
        const b = Buffer.from(r.body);
        return { initRc: b.readInt32BE(0), puts: b.readUInt32BE(4), putRc: b.readInt32BE(8), value: b.readUInt32BE(12), status: b.readUInt32BE(28) };
      } catch (e) {
        lastRefusal = String(e);
        return null;
      }
    }
    /** The fixture's state once it answers (it may still be starting). */
    const loadedState = async (rt: TestRuntime) =>
      (await pollUntil(rt.clock, () => userState(rt), (s) => s !== null, {
        withinMs: WITHIN_MS,
        message: () => `${USER} answers its read command (last: ${lastRefusal})`,
      }))!;
    /** The module puts `value` under its key (the UI task runs it) and says so. */
    async function putValue(rt: TestRuntime, value: number): Promise<void> {
      const before = await pollUntil(rt.clock, () => userState(rt), (s) => s !== null, {
        withinMs: WITHIN_MS,
        message: `${USER} is loaded`,
      });
      const reply = await run.usb(rt, (dk) => dk.request(CMD_PUT, word32(value)));
      expect(reply.status, 'the put is queued').toBe(0);
      const after = await pollUntil(rt.clock, () => userState(rt), (s) => s !== null && s.puts > before!.puts, {
        withinMs: WITHIN_MS,
        message: 'the put ran',
      });
      expect(after!.putRc, 'the put was taken').toBe(0);
    }
    /** The module was (re)loaded and its `init` read `value` under its key: the project's own value is in the overlay. */
    async function expectSeen(rt: TestRuntime, value: number, why: string): Promise<void> {
      const s = await pollUntil(rt.clock, () => userState(rt), (v) => v !== null && v.initRc === 4 && v.value === value, {
        withinMs: WITHIN_MS,
        message: `${why}: ${USER} read 0x${value.toString(16)}`,
      });
      expect(s, why).toMatchObject({ initRc: 4, value });
    }
    /** DkrtBootInfo.reloads (offset 252): moves when a live job finished. */
    const reloadsAt = (rt: TestRuntime) => word(rt.session, planSymbol(run.plan, 'DKRT_BOOT') + 252);
    /** The files of the store below `projects/` that are project files (not the archive's folder). */
    const dkpPaths = async (rt: TestRuntime) => (await projectPaths(usbOf(rt))).filter((p) => p.endsWith('.dkp'));
    /** The event with its slot, bytes and result checked: a save or a load that has data. */
    const expectSaved = (e: { args: number[] }, slot: number, why: string) => {
      expect(e.args[0], `${why}: the slot`).toBe(slot);
      expect(e.args[1], `${why}: the file has bytes`).toBeGreaterThan(0);
      expect(e.args[3], `${why}: the result is ok`).toBe(0);
    };
    const expectNoIgnored = (log: ProjectLog, since: number, why: string) =>
      expect(log.all('PROJECT_IGNORED', () => true, since), `${why}: no file was ignored`).toEqual([]);

    test(
      'a project follows the stock menus: first project, save, save as, load, restore of a machine, rename, copy, clear',
      async () => {
        const rt = await run.load('project', spec());
        const usb = usbOf(rt);
        const modulePath = run.mod(SINE).name;
        const log = await ProjectLog.openBoot(usb, rt.clock, [USER]);

        // C: the boot's stock project is in no slot (the card holds no saved project): the service does nothing, no project event, the module is not reloaded
        await log.until('STABLE', () => true, { what: 'STABLE at the start' });
        expect(log.all('PROJECT_LOADED').concat(log.all('PROJECT_IGNORED'), log.all('PROJECT_SAVED')), 'C: no project event at the start').toEqual([]);
        expect(await reloadsAt(rt), 'C: no module was reloaded').toBe(0);
        expect((await run.usb(rt, (dk) => dk.modules())).map((m) => m.name), `C: the fixture and the machine are loaded (loads: ${JSON.stringify(log.all('MOD_LOAD').map((e) => e.fields))}; commits: ${JSON.stringify(log.all('MODULE_COMMIT').map((e) => e.fields))})`).toEqual(expect.arrayContaining([USER, SINE]));
        expect((await loadedState(rt)).initRc, 'C: the module found no value').toBe(E_SET_NOKEY);
        expect(await dkpPaths(rt), 'C: no project file yet').toEqual([]);

        const artifacts = liveArtifacts(rt.session, 'project-service');
        const snap = (tag: string) => shot(rt.session, artifacts, tag);

        await rt.session.expectNoFault('after case C');
        // A: the first save gives the project its slot (a value put before that is the settings record's alone); a value, a second save: the file of the slot holds it
        expectSaved((await savePanelProject(rt, { events: log, from: SLOT_FIRST }))!, SLOT_FIRST, 'A first save');
        await putValue(rt, VALUE_A);
        expectSaved((await savePanelProject(rt, { events: log, from: SLOT_FIRST }))!, SLOT_FIRST, 'A save with the value');
        await snap('a-saved');
        const afterA = await dkpPaths(rt);
        const first = slotFile(afterA, SLOT_FIRST);
        expect(first, `A: projects/001-<name>.dkp is in ${JSON.stringify(afterA)}`).toBeDefined();
        const name = nameOfFile(first!);

        await rt.session.expectNoFault('after case A');
        // B: another value, save as slot 2; then load slot 1: the module reads the first value again
        await putValue(rt, VALUE_B);
        await snap('b-before-save-as');
        expectSaved(
          (await saveProjectAs(rt, SLOT_SECOND, { events: log, from: SLOT_FIRST }).catch(async (e: Error) => {
            await snap('b-failed');
            throw e;
          }))!,
          SLOT_SECOND,
          'B save as',
        );
        const afterB = await dkpPaths(rt);
        expect(slotFile(afterB, SLOT_SECOND), `B: slot 2 has its file in ${JSON.stringify(afterB)}`).toBeDefined();
        expect(slotFile(afterB, SLOT_FIRST), 'B: the file of slot 1 stays').toBe(first);
        const loadedB = await loadProject(rt, SLOT_FIRST, { events: log, from: SLOT_SECOND, saved: [SLOT_FIRST, SLOT_SECOND] });
        expect(loadedB!.args[0], 'B: slot 1 was loaded').toBe(SLOT_FIRST);
        expect(loadedB!.args[1], 'B: its file has bytes').toBeGreaterThan(0);
        await expectSeen(rt, VALUE_A, 'B: after the load of slot 1');

        await rt.session.expectNoFault('after case B');
        // D: the machine is unloaded, slot 2 is loaded: the service loads it again, keeps it in the selection, and the module reads the second value
        await unloadLive(rt, usb, run.plan, modulePath);
        expect((await run.usb(rt, (dk) => dk.modules())).map((m) => m.name), 'D: the machine is gone').not.toContain(SINE);
        const sinceD = await log.mark();
        await log.raise();
        await loadProject(rt, SLOT_SECOND, { events: log, from: SLOT_FIRST, saved: [SLOT_FIRST, SLOT_SECOND] });
        const sineHash = idHash(SINE);
        const isSine = (what: number) => (e: { args: number[] }) => e.args[1] === what && e.args[4]! >>> 0 === sineHash;
        await log.until('PROJECT_MODULE', isSine(1), { since: sinceD, what: 'PROJECT_MODULE queued for the machine' });
        const done = await log.until('PROJECT_MODULE', isSine(2), { since: sinceD, what: 'PROJECT_MODULE done for the machine' });
        expect(done.args[2], 'D: the machine was added to the selection').toBe(0);
        expect((await run.usb(rt, (dk) => dk.modules())).map((m) => m.name), 'D: the machine is loaded').toContain(SINE);
        expect(currentRecord(await run.records())?.paths, 'D: it is in the saved selection').toContain(modulePath);
        await log.raise();
        await expectSeen(rt, VALUE_B, 'D: after the load of slot 2');
        expectNoIgnored(log, 0, 'D');

        await rt.session.expectNoFault('after case D');
        // F: rename the current project (slot 2), then save: the file follows the name, it is no new project (nothing archived)
        const oldPath = slotFile(await dkpPaths(rt), SLOT_SECOND)!;
        await renameProject(rt, SLOT_SECOND);
        const sinceF = await log.mark();
        expectSaved((await savePanelProject(rt, { events: log, from: SLOT_SECOND }))!, SLOT_SECOND, 'F save after rename');
        const afterF = await dkpPaths(rt);
        const renamed = slotFile(afterF, SLOT_SECOND);
        expect(renamed, `F: slot 2 has a file in ${JSON.stringify(afterF)}`).toBeDefined();
        expect(renamed, 'F: the file has the new name').not.toBe(oldPath);
        expect(afterF, 'F: the old name is gone').not.toContain(oldPath);
        expect(afterF.filter((p) => p.startsWith(ARCHIVE)), `F: nothing was archived (files ${JSON.stringify(afterF)}; events ${log.summary(sinceF)})`).toEqual([]);
        expectNoIgnored(log, sinceF, 'F');

        await rt.session.expectNoFault('after case F');
        // E: copy slot 1 to slot 3: a file with the same sections; clear slot 3: its file moves to the archive
        await copyPasteProject(rt, SLOT_FIRST, SLOT_COPY);
        const copyPath = (
          await pollUntil(rt.clock, () => dkpPaths(rt), (p) => slotFile(p, SLOT_COPY) !== undefined, {
            withinMs: WITHIN_MS,
            message: 'E: the pasted file of slot 3 appears',
          })
        ).find((p) => slotFile([p], SLOT_COPY))!;
        const source = dkpOf(await run.usb(rt, (dk) => readStoreFile(dk, first!)));
        const copy = dkpOf(await run.usb(rt, (dk) => readStoreFile(dk, copyPath)));
        expect(nameOfFile(copyPath), 'E: the copy keeps the name').toBe(name);
        expect({ slot: copy.slot, counter: copy.counter, stamp: copy.stamp }, 'E: the copy has its own slot and counter, the stamp of the source').toEqual({
          slot: SLOT_COPY,
          counter: 1,
          stamp: source.stamp,
        });
        expect(copy.sections.equals(source.sections), 'E: the copy has the sections of the source').toBe(true);
        await rt.session.expectNoFault('after the paste');
        const sinceClear = await log.mark();
        await clearProject(rt, SLOT_COPY);
        await snap('e-cleared');
        const archived = `${ARCHIVE}${name}-${hex8(copy.stamp)}.dkp`;
        const afterClear = await pollUntil(rt.clock, () => projectPaths(usb), (p) => p.includes(archived), {
          withinMs: WITHIN_MS,
          message: () => `E: ${archived} appears (events ${log.summary(sinceClear)})`,
        });
        expect(slotFile(afterClear, SLOT_COPY), 'E: slot 3 has no file after the clear').toBeUndefined();

        await rt.session.expectNoFault('the project service flows');
      },
      1_800_000,
    );

    // No device case for the first bind at a start: the emulator's restart does not keep the firmware's persisted project record (the slot byte reads -1 after it, no
    // project is current), so the precondition cannot be made here. The first bind is covered by the service's unit tests; what the slot byte holds at a real boot stays [D].
  },
);
