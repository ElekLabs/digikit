import { expect, test } from 'vitest';
import { DkError } from '@digi/dk-session';
import { pollUntil } from '@digi/test-support';
import { CARD_SYMBOLS } from '@digi/regression/runtime/card-symbols.ts';
import { cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { RT } from '@digi/regression/runtime/runtime-roots.ts';
import { type CardRig } from '@digi/regression/runtime/card-rig-api.ts';
import {
  dkpOf,
  ProjectLog,
  projectPaths,
  readStoreFile,
  savePanelProject,
  slotFile,
  type ProtocolRun,
} from '@digi/regression/runtime/project-kit.ts';
import { word } from '@digi/regression/runtime/guest-access.ts';
import { bootReloads } from '@digi/regression/runtime/guest-mailbox.ts';
import type { TestRuntime } from '@digi/regression/test-runtime.ts';

/**
 * Hot reload of the `project` service (hot reload slice 2, the State part): `RELOAD services/project.dks` takes `test/project-user` and the service down, carries the service's state
 * (binding, the live image with its unsaved value, dirty flag, epoch, module-restore state, the hooks' record) to the new copy and brings both back. The state carry, its refusals and the
 * order of the steps are unit and assembly tests (`services/project/tests/carry.rs`, `state_reload_assembly_tests.rs`); the cases here show what only the firmware shows:
 * - DF-S1: after the reload the project has the same slot and name, is dirty, the module reads the unsaved value, and a stock save (the hooks of the new copy) writes it;
 * - DF-S2: a stock save started while the job runs is saved: the service refuses its take-down (`can_stop`: E_BUSY) while hook work is queued, and a save that starts later runs on the new copy's carried hooks, so the file holds the value and the job ends well;
 * - DF-S3: no module is live-loaded again (no PROJECT_MODULE, the same module list);
 * - DF-S5: a module that needs the project service is refused at run time with E_BOOT_ONLY when the service was not loaded at the boot (the first live load).
 * Not driven here: carried bytes of an older layout (DF-S4). A second file of the service in an older layout is a build variant of the crate (`packModuleVariant` copies one module
 * directory, the service crate lives beside it), and the stock project menus never produce such bytes. The drop of refused bytes (named, unbound, one log line, one toast) is the unit
 * test `a_first_start_is_unbound_and_silent_and_dropped_state_is_one_toast_and_one_log_line`.
 */
const USER = 'test/project-user';
const SINE = 'test/sine';
const DEBUG = 'digikit/debug';
const SERVICE = 'services/project.dks';
const SLOT = 0;
const VALUE_A = 0xa1;
const VALUE_B = 0xb2;
const VALUE_C = 0xc3;
const CMD_PUT = 0x38;
const CMD_READ = 0x39;
const STABLE_BLOCKS = 300;
const WITHIN_MS = 30_000;
/** A hang guard of the whole reload (wall time); the job's end is what the wait is for. */
const RELOAD_WITHIN_MS = 1_800_000;
const E_BOOT_ONLY = -45;
/** `dk_project_status` bits: 0 file loaded, 1 dirty. */
const BIT_LOADED = 1;
const BIT_DIRTY = 2;

const word32 = (v: number) => {
  const b = Buffer.alloc(4);
  b.writeUInt32BE(v);
  return b;
};

cardRigs(
  'project service reload',
  { dsp: 'sharc', roots: RT, symbols: CARD_SYMBOLS, initial: () => ({}), project: true, uploads: true, relaxFs: true, stallMs: 2000 },
  (run: CardRig) => {
    const spec = (selection = [DEBUG, USER, SINE]) => ({
      modules: [DEBUG, USER, SINE],
      selection: selection.map((id) => run.mod(id).name),
      stableBlocks: STABLE_BLOCKS,
    });
    const usbOf =
      (rt: TestRuntime): ProtocolRun =>
      (f) =>
        run.usb(rt, f);
    async function userState(rt: TestRuntime) {
      try {
        const r = await run.usb(rt, (dk) => dk.request(CMD_READ));
        if (r.status !== 0) return null;
        const b = Buffer.from(r.body);
        return { initRc: b.readInt32BE(0), puts: b.readUInt32BE(4), value: b.readUInt32BE(12), status: b.readUInt32BE(28) };
      } catch (e) {
        // a refusal the device reports (the module is not there yet); a USB failure or a fault is not one
        if (e instanceof DkError) return null;
        throw e;
      }
    }
    const answers = async (rt: TestRuntime, ok: (s: NonNullable<Awaited<ReturnType<typeof userState>>>) => boolean, message: string) =>
      (await pollUntil(rt.clock, () => userState(rt), (s) => s !== null && ok(s), { withinMs: WITHIN_MS, message }))!;
    async function putValue(rt: TestRuntime, value: number): Promise<void> {
      const before = await answers(rt, () => true, `${USER} is loaded`);
      expect((await run.usb(rt, (dk) => dk.request(CMD_PUT, word32(value)))).status, 'the put is queued').toBe(0);
      const after = await answers(rt, (s) => s.puts > before.puts, 'the put ran');
      void after;
    }
    const modules = async (rt: TestRuntime) => (await run.usb(rt, (dk) => dk.modules())).map((m) => m.name).sort();
    const reloadsAt = (rt: TestRuntime) => bootReloads(rt.session, run.plan);
    const fileOf = async (rt: TestRuntime) => {
      const path = slotFile(await projectPaths(usbOf(rt)), SLOT)!;
      return { path, file: dkpOf(await run.usb(rt, (dk) => readStoreFile(dk, path))) };
    };
    /** A project with its first save, the first value saved, the second put and not saved. */
    async function bootDirty(tag: string) {
      const rt = await run.load(tag, spec());
      const log = await ProjectLog.openBoot(usbOf(rt), rt.clock, [USER]);
      await log.until('STABLE', () => true, { what: 'STABLE at the start' });
      await savePanelProject(rt, { events: log, from: SLOT });
      await putValue(rt, VALUE_A);
      await savePanelProject(rt, { events: log, from: SLOT });
      const saved = await fileOf(rt);
      expect(saved.file.sections.includes(word32(VALUE_A)), 'the saved file holds the first value').toBe(true);
      await putValue(rt, VALUE_B);
      expect((await answers(rt, () => true, 'status')).status & BIT_DIRTY, 'the project is dirty before the reload').toBe(BIT_DIRTY);
      return { rt, log, saved };
    }

    test('DF-S1 and DF-S3: RELOAD of the project service keeps slot, name and the unsaved value; a stock save writes it; no module is loaded again', async () => {
      const { rt, log, saved } = await bootDirty('project-reload');
      const modulesBefore = await modules(rt);
      const reloadsBefore = await reloadsAt(rt);
      const since = await log.mark();
      const end = await run.usb(rt, (dk) => dk.reloadAndWait(SERVICE, { withinMs: RELOAD_WITHIN_MS })).catch(async (e: Error) => {
        await log.pump();
        const steps = log.all('LIFECYCLE', () => true, since).slice(0, 40).map((x) => JSON.stringify(x.fields));
        throw new Error(`${e.message}; first lifecycle events: ${steps.join(' ')}`);
      });
      expect({ result: end.result, unloaded: end.unloaded }, 'the reload of the project service ran to its end').toEqual({ result: 0, unloaded: true });
      expect(await reloadsAt(rt), 'the job counter moved').toBeGreaterThan(reloadsBefore);

      // the module came back with the project's live image: the unsaved value, bound and dirty
      const after = await answers(rt, (s) => s.initRc === 4 && s.value === VALUE_B, 'the module reads the unsaved value');
      expect(after.status & (BIT_LOADED | BIT_DIRTY), 'bound and still dirty').toBe(BIT_LOADED | BIT_DIRTY);
      await log.pump();
      expect(log.all('PROJECT_MODULE', () => true, since), 'DF-S3: no module restore ran').toEqual([]);
      expect(log.all('PROJECT_IGNORED', () => true, since), 'no file was ignored, no carry was dropped').toEqual([]);
      expect(await modules(rt), 'DF-S3: the same modules are loaded').toEqual(modulesBefore);

      // a stock save reaches the new copy's hooks and writes the value to the same file
      const event = await savePanelProject(rt, { events: log, from: SLOT });
      expect(event!.args[3], 'the save is ok').toBe(0);
      const now = await fileOf(rt);
      expect(now.path, 'the same slot and name').toBe(saved.path);
      expect(now.file.sections.includes(word32(VALUE_B)), 'the file holds the unsaved value').toBe(true);
      expect(now.file.counter, 'one more write').toBe(saved.file.counter + 1);
      await rt.session.expectNoFault('project reload');
    }, 1_800_000);

    test('DF-S2: a stock save during the job is saved, and the job ends well', async () => {
      const { rt, log, saved } = await bootDirty('project-reload-save');
      await run.usb(rt, (dk) => dk.reload(SERVICE));
      // the save sequence runs through the panel while the job is queued or running; the job's end is read on the emulated clock, one request each (the save needs the panel between the two)
      const event = await savePanelProject(rt, { events: log, from: SLOT });
      expect(event!.args[3], 'the save is ok').toBe(0);
      const end = await pollUntil(rt.clock, () => run.usb(rt, (dk) => dk.reloadState()), (s) => s.phase === 2, {
        withinMs: WITHIN_MS,
        message: 'the job ended',
      });
      expect(end.result, 'the job ended well').toBe(0);
      const now = await fileOf(rt);
      expect(now.path, 'the same slot and name').toBe(saved.path);
      expect(now.file.sections.includes(word32(VALUE_B)), 'the saved file holds the value').toBe(true);
      await answers(rt, (s) => s.initRc === 4, 'the module is back');
      await putValue(rt, VALUE_C);
      await savePanelProject(rt, { events: log, from: SLOT });
      expect((await fileOf(rt)).file.sections.includes(word32(VALUE_C)), 'a later save through the new copy writes a later value').toBe(true);
      await rt.session.expectNoFault('project reload during a save');
    }, 1_800_000);

    test('DF-S5: a module that needs the project service is refused with E_BOOT_ONLY when the service did not start at the boot', async () => {
      const rt = await run.load('project-live-first', spec([DEBUG, SINE]));
      const log = await ProjectLog.openBoot(usbOf(rt), rt.clock, []);
      await log.until('STABLE', () => true, { what: 'STABLE at the start' });
      const before = await modules(rt);
      expect(before, 'the project user is not loaded').not.toContain(USER);
      const end = await run.usb(rt, (dk) => dk.loadAndWait(run.mod(USER).name));
      expect(end.result, 'the first live load of the project service is refused').toBe(E_BOOT_ONLY);
      expect(await modules(rt), 'nothing was loaded').toEqual(before);
      await rt.session.expectNoFault('first live load');
    }, 1_800_000);
  },
);
