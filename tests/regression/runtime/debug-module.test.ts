import { hasCap } from '@digi/dk-session';
import { LOG_KIND, SERVICE_CMD } from '@digi/dk-session/gen_proto';
import { expect } from 'vitest';
import type { PatchedSession } from '../support.ts';
import {
  DEBUG_MODULE,
  installModules,
  installWithDebug,
  liveArtifacts,
  moduleArtifact,
  runtimeTests,
  settleUi,
} from './live.ts';
import { readSmallRing, RING_SYMBOLS } from './runtime-menu.ts';
import { prepareRuntimePlan } from './runtime-plan.ts';

const be32 = (n: number) => Uint8Array.of(n >>> 24, (n >>> 16) & 255, (n >>> 8) & 255, n & 255);
const text = (words: number[]) =>
  String.fromCharCode(...words.flatMap((w) => [w >>> 24, (w >>> 16) & 255, (w >>> 8) & 255, w & 255]).filter(Boolean));

/** Without digikit/debug the runtime boots and works, has no log commands, and keeps its small ring of boot and error events. */
async function expectWithoutDebugModule(runtime: PatchedSession): Promise<void> {
  const { session, dk } = runtime;
  await settleUi(session);
  const plan = await prepareRuntimePlan(session, liveArtifacts(session, 'debug-module'), RING_SYMBOLS);
  const info = await dk.info();
  expect(hasCap(info, 'SERVICE_CMDS'), 'a runtime that answers').toBe(true);
  for (const cap of ['LOG', 'PEEK', 'LOGCFG'] as const) expect(hasCap(info, cap), `no ${cap} capability`).toBe(false);
  for (const cmd of [SERVICE_CMD.LOG, SERVICE_CMD.PEEK, SERVICE_CMD.LOGCFG])
    expect((await dk.request(cmd, new Uint8Array(6))).status, `command ${cmd.toString(16)} is unknown`).toBe(-60);
  expect(await dk.modules()).toEqual([]);

  const ring = await readSmallRing(session, plan);
  const kinds = ring.map((e) => e.kind);
  expect(kinds, 'the boot events are in the small ring').toEqual(
    expect.arrayContaining([LOG_KIND.BOOT_START, LOG_KIND.UI_FIRST]),
  );
  expect(ring.length, 'the ring holds at most 32 events').toBeLessThanOrEqual(32);
  // only error events and boot events are kept: no periodic HEALTH, DSP_DIAG or MOD_LOG
  for (const kind of [LOG_KIND.HEALTH, LOG_KIND.DSP_DIAG, LOG_KIND.MOD_LOG]) expect(kinds).not.toContain(kind);
  await session.expectNoFault('runtime without the debug module');
}

/** With it: the boot events are there (handed over from the small ring), levels are read and set, PEEK reads RAM inside the device ranges and refuses the rest,
 * and a module's own log line (digikit/probe logs `probe ready` in init) arrives with the module as its source. */
async function expectWithDebugModule(runtime: PatchedSession): Promise<void> {
  const { session, dk } = runtime;
  await settleUi(session);
  await installWithDebug(runtime, 'digikit/probe');
  const info = await dk.info();
  for (const cap of ['LOG', 'PEEK', 'LOGCFG'] as const) expect(hasCap(info, cap), `${cap} capability`).toBe(true);

  const modules = await dk.modules();
  const probe = modules.find((m) => m.name === 'digikit/probe')!;
  expect(modules.map((m) => m.name)).toEqual(['digikit/debug', 'digikit/probe']);
  await expect
    .poll(async () => (await dk.readLog()).some((e) => e.kind === LOG_KIND.UI_FIRST), {
      timeout: 60_000,
      interval: 1000,
    })
    .toBe(true);
  const log = await dk.readLog();
  const first = (kind: number) => log.find((e) => e.kind === kind);
  expect(first(LOG_KIND.BOOT_START), 'the boot events were handed over').toBeDefined();
  expect(log.filter((e) => e.kind === LOG_KIND.MOD_LOAD)).toHaveLength(2);
  expect(first(LOG_KIND.BOOT_START)!.seq, 'in order').toBeLessThan(first(LOG_KIND.UI_FIRST)!.seq);
  const seqs = log.map((e) => e.seq);
  expect(seqs, 'sequence numbers count up by one').toEqual(seqs.map((_, i) => seqs[0]! + i));

  const modLog = log.find((e) => e.kind === LOG_KIND.MOD_LOG && text(e.args.slice(0, 4)).startsWith('probe ready'));
  expect(modLog, 'the module log line arrives').toBeDefined();
  expect((modLog!.flags >> 8) & 0xff, 'its source is the probe module (record index + 1)').toBe(probe.index + 1);

  // levels: the defaults, then a set that is read back; the levels are in RAM
  expect(await dk.logConfig()).toEqual({ level: 2, modules: Array(8).fill(0xff), debug: true });
  const modulesLevels = Array<number>(8).fill(0xff);
  modulesLevels[probe.index] = 4;
  expect(await dk.setLogConfig(1, modulesLevels)).toEqual({ level: 1, modules: modulesLevels, debug: true });
  expect(await dk.logConfig()).toEqual({ level: 1, modules: modulesLevels, debug: true });
  expect((await dk.request(SERVICE_CMD.LOGCFG, Uint8Array.of(2, 1))).status, 'the old debug switch is gone').toBe(-60);

  // PEEK: the first range of the device file starts at the firmware's data
  const ok = await dk.request(SERVICE_CMD.PEEK, new Uint8Array([...be32(0x40000000), 0, 16]));
  expect(ok.status).toBe(0);
  expect(ok.body.length).toBe(6 + 16);
  expect([...ok.body.subarray(0, 6)]).toEqual([0x40, 0, 0, 0, 0, 16]);
  expect((await dk.request(SERVICE_CMD.PEEK, new Uint8Array([...be32(0x10), 0, 4]))).status, 'outside the ranges').toBe(
    -63,
  );
  expect(
    (await dk.request(SERVICE_CMD.PEEK, new Uint8Array([...be32(0x40000000), 2, 1]))).status,
    'over 512 bytes',
  ).toBe(-63);
  await session.expectNoFault('runtime with the debug module');
}

/** A module that loads before digikit/debug and logs in its init (digikit/remote: `remote registered`): the small ring kept the line through the boot, and
 * the debug module copies it when it starts. */
async function expectEarlyModuleLog(runtime: PatchedSession): Promise<void> {
  const { session, dk } = runtime;
  await settleUi(session);
  const device = session.options.device;
  await installModules(
    dk,
    ['digikit/remote', DEBUG_MODULE].map((id) => ({ id, artifact: moduleArtifact(device, id) })),
  );
  expect((await dk.modules()).map((m) => m.name)).toEqual(['digikit/remote', 'digikit/debug']);
  await expect
    .poll(async () => (await dk.readLog()).some((e) => e.kind === LOG_KIND.UI_FIRST), {
      timeout: 60_000,
      interval: 1000,
    })
    .toBe(true);
  const log = await dk.readLog();
  const line = log.find((e) => e.kind === LOG_KIND.MOD_LOG && text(e.args.slice(0, 4)).startsWith('remote registere'));
  expect(line, 'the log line of the module that loaded before the debug module').toBeDefined();
  expect(log.find((e) => e.kind === LOG_KIND.BOOT_START)!.seq).toBeLessThan(line!.seq);
  await session.expectNoFault('runtime with an early module log line');
}

runtimeTests(
  'without the debug module the runtime boots, has no log commands and keeps a small ring',
  expectWithoutDebugModule,
  {
    timeout: 600_000,
  },
);
runtimeTests('with the debug module the boot events, levels, PEEK and a module log line work', expectWithDebugModule, {
  timeout: 900_000,
});
runtimeTests('a module that loads before the debug module has its log line in the debug log', expectEarlyModuleLog, {
  timeout: 900_000,
});
