import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { expect } from 'vitest';
import type { TestRuntime } from '@digi/regression/support.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { DEBUG_MODULE } from '@digi/regression/runtime/install.ts';
import { loadCode } from '@digi/regression/runtime/load-states.ts';
import { runtimeTests } from '@digi/regression/runtime/ready.ts';
import { cliInstall } from '@digi/regression/runtime/transfer-cli.ts';

/**
 * Reload of a content module (docs/plans/extension-model.md E5): `RELOAD` swaps the code, DSP and keys of a loaded module without a boot.
 *
 * - (a) a code-only module (`test/lfo-shape`) reloads: no new epoch, the module is listed again, no table fault is logged.
 * - (b) a machine with DSP (`test/sine`) reloads with a note held: its machine type keeps its number, its voice row is the same row, the module is listed again,
 *   and the window is silent (the placeholder row of the type: no stock sound) until the new row is published.
 * - (c) the reload is refused while the transport plays (`E_RELOAD_PLAYING`), leaves the old module live, and goes through after STOP.
 * - the soak: the reload of the machine, 5 cycles in the gate and 20 in the full tier (`DIGIKIT_TIER=full`), each cycle checks that no module record, voice
 *   row or arena byte is lost, that the machine type does not move and that no table fault or module fault is logged. The pitch switch of the
 *   plan's soak (two builds of `test/sine` that differ in one constant) needs a build helper with a changed constant and is still to be written.
 *
 * Run only with the go of the owner (device tests: docs/plans/extension-model.md, "Device proof").
 */
const MODULE_PATH = 'modules/sine.dkm';
const SHAPE_PATH = 'modules/lfo-shape.dkm';
const E_RELOAD_PLAYING = loadCode('E_RELOAD_PLAYING');
const SOAK_CYCLES = process.env.DIGIKIT_TIER === 'full' ? 20 : 5;

const moduleRows = async (rt: TestRuntime) => (await rt.dk.modules()).filter((m) => m.name !== DEBUG_MODULE);

/** The epoch of the daemon's guest: a boot makes a new one. */
const epochOf = async (rt: TestRuntime): Promise<number> => {
  const e = (await rt.session.status()).epoch;
  if (typeof e !== 'number') throw new Error('daemon status must report a numeric epoch');
  return e;
};

/** No table fault (`TABLE_BAD`) and no module fault in the log of this boot. */
async function expectNoFault(rt: TestRuntime, why: string): Promise<void> {
  const events = await rt.dk.readLog();
  expect(events.filter((e) => e.kind === LOG_KIND.TABLE_BAD), `${why}: a table check failed`).toEqual([]);
}

async function expectReloadLive(rt: TestRuntime): Promise<void> {
  const artifacts = liveArtifacts(rt.session, 'reload-live');
  await cliInstall(rt.session, 'test/lfo-shape', artifacts, 300_000, [DEBUG_MODULE, 'test/sine']);
  const epoch = await epochOf(rt);
  const before = await moduleRows(rt);
  record(artifacts, 'modules-before', before);

  // (a) a code-only module
  expect(await rt.dk.reloadAndWait(SHAPE_PATH), 'the reload of the code-only module').toBe(0);
  expect(await epochOf(rt), 'no boot').toBe(epoch);
  const afterShape = await moduleRows(rt);
  expect(afterShape.map((m) => m.name).sort(), 'the modules are the same').toEqual(before.map((m) => m.name).sort());
  await expectNoFault(rt, 'after the code-only reload');

  // (b) the machine with DSP, a note held
  const sine = before.find((m) => m.name === 'test/sine')!;
  await rt.ui.tap('TRIG 1', { until: { ms: 200, why: 'a note on the track' } });
  expect(await rt.dk.reloadAndWait(MODULE_PATH), 'the reload of the machine').toBe(0);
  expect(await epochOf(rt), 'no boot').toBe(epoch);
  const again = (await moduleRows(rt)).find((m) => m.name === 'test/sine')!;
  record(artifacts, 'sine-after', again);
  expect(again.types, 'the machine type keeps its number').toEqual(sine.types);
  expect(again.rows, 'the voice row is the one it had (the placeholder row is taken over)').toBe(sine.rows);
  await expectNoFault(rt, 'after the machine reload');

  // (c) refused while the transport plays; the old module stays
  await rt.ui.tap('PLAY', { until: { ms: 300, why: 'the transport starts' } });
  expect(await rt.dk.reloadAndWait(MODULE_PATH), 'the transport plays').toBe(E_RELOAD_PLAYING);
  expect((await moduleRows(rt)).find((m) => m.name === 'test/sine')?.types, 'the old module is live').toEqual(sine.types);
  await rt.ui.tap('STOP', { until: { ms: 300, why: 'the transport stops' } });
  await rt.ui.tap('STOP', { until: { ms: 300, why: 'the second STOP resets the transport' } });
  expect(await rt.dk.reloadAndWait(MODULE_PATH), 'after STOP').toBe(0);
  expect(await epochOf(rt), 'still no boot').toBe(epoch);
}

async function expectReloadSoak(rt: TestRuntime): Promise<void> {
  const artifacts = liveArtifacts(rt.session, 'reload-soak');
  await cliInstall(rt.session, 'test/sine', artifacts, 300_000, [DEBUG_MODULE]);
  const epoch = await epochOf(rt);
  const start = await rt.dk.capacity();
  const sine = (await moduleRows(rt)).find((m) => m.name === 'test/sine')!;
  for (let cycle = 0; cycle < SOAK_CYCLES; cycle++) {
    expect(await rt.dk.reloadAndWait(MODULE_PATH), `cycle ${cycle}`).toBe(0);
    const cap = await rt.dk.capacity();
    const m = (await moduleRows(rt)).find((x) => x.name === 'test/sine')!;
    expect(m.types, `cycle ${cycle}: the machine type moved`).toEqual(sine.types);
    expect(m.rows, `cycle ${cycle}: the voice row moved`).toBe(sine.rows);
    expect(m.faults, `cycle ${cycle}: the guard counted a fault`).toBe(0);
    for (const kind of ['MODULES', 'MACHINE_ROWS', 'VOICE_ROWS', 'PARAM_ROWS', 'DSP_AREA', 'ARENA']) {
      expect(cap[kind]?.used, `cycle ${cycle}: ${kind} in use grew`).toBe(start[kind]?.used);
    }
    await expectNoFault(rt, `cycle ${cycle}`);
  }
  expect(await epochOf(rt), 'the soak never booted').toBe(epoch);
  record(artifacts, 'soak', { cycles: SOAK_CYCLES, capacity: await rt.dk.capacity() });
}

runtimeTests('RELOAD swaps a code-only module and a machine without a boot, and refuses while the transport plays', expectReloadLive, { dsp: 'sharc' });
runtimeTests('RELOAD of a machine leaves no row, record or byte behind over many cycles', expectReloadSoak, { dsp: 'sharc' });
