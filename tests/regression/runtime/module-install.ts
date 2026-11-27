import fs from 'node:fs';
import type { DkSession } from '@digi/dk-session';
import { poll, RuntimeEvents } from '@digi/dk-session';
import { moduleStorePathOf, type Device, type Session } from '@digi/test-support';
import { expect } from 'vitest';
import { timePhase } from '../timing.ts';
import { DEBUG_MODULE } from './debug-module.ts';
import { moduleArtifact } from './module-artifact.ts';
import { neededServices, portFile, requireCoupledForServices, servicePath } from './service-files.ts';

/** Emulated milliseconds the boot-time loads of the installed modules may take after the boot is seen. */
const MODULES_LOADED_EMULATED_MS = 30_000;

/**
 * Native USB upload of prepared modules, one selection with all of them (in this order), one restart, then wait for their registration.
 * With `device`, the services that the modules need (`patchkit services`: `fx.dks` for an effect) go on the card too, bound to the stock firmware (the Nx target
 * `<service id>:bind`; the runtime finds them by name at boot). With the debug module in the list, `dk.waitForBoot()` ends on the log's FW_READY: the boot intro
 * is over and the stock MidiRpc service answers (without the log, the session's own probe before the first stock request covers it).
 */
export async function installModules(
  dk: DkSession,
  modules: { id: string; artifact: string }[],
  { device, session }: { device?: Device; session?: Pick<Session, 'status' | 'client'> } = {},
): Promise<void> {
  await timePhase(
    'module.install',
    async () => {
      const paths: string[] = [];
      for (const { id, artifact } of modules) {
        const bytes = fs.readFileSync(artifact);
        const storePath = moduleStorePathOf(id, bytes);
        await timePhase('module.upload', () => dk.put(storePath, bytes));
        expect(await dk.verifyFile(storePath, bytes), 'fresh card read matches prepared module bytes').toBe(true);
        paths.push(storePath);
      }
      if (device) {
        const services = neededServices(modules.map((m) => m.artifact));
        if (services.length) {
          // the port file the services resolve their firmware names from
          const port = portFile(device);
          const bytes = fs.readFileSync(port.file);
          await timePhase('service.upload', () => dk.put(port.name, bytes));
          expect(await dk.verifyFile(port.name, bytes), 'fresh card read matches the port file').toBe(true);
        }
        for (const needed of services) {
          const name = needed.service;
          const boundFile = moduleArtifact(device, needed.moduleId);
          // the rule holds whenever a service goes on the card: a call without a session cannot show the mode, so it is refused for a service with a DSP part
          requireCoupledForServices(
            [{ name: servicePath(name), file: boundFile }],
            session ? (await session.status()).dsp : undefined,
          );
          const packed = fs.readFileSync(boundFile);
          await timePhase('service.upload', () => dk.put(servicePath(name), packed));
          expect(
            await dk.verifyFile(servicePath(name), packed),
            `fresh card read matches the packed ${name} service`,
          ).toBe(true);
        }
      }
      await dk.select(paths);
      await timePhase('protocol.restart', () => dk.restart());
      // waitForBoot ends at FW_READY with the debug module (the log), after the stock MidiRpc service answers
      await timePhase('protocol.waitForBoot', () => dk.waitForBoot());
      const want = modules.map((m) => m.id);
      if (want.includes(DEBUG_MODULE)) {
        // the debug module's log is there: the runtime's own events say when each module of the selection is in (MOD_LOAD, and MOD_DSP for a DSP part), no list poll
        const events = new RuntimeEvents(dk, dk.clock);
        await events.listen({ fromBoot: true });
        // a DSP part completes (MOD_DSP) only when the session runs the SHARC DSP: without it the CF part (MOD_LOAD) is final
        const dsp = session ? (await session.status()).dsp === 'sharc' : true;
        await events.untilModulesActive(modules.length, { withinMs: MODULES_LOADED_EMULATED_MS * 4, dsp });
        return;
      }
      const loaded = async () => (await dk.modules()).map((entry) => entry.name);
      // the module loads (DSP parts take many blocks) run on the session's clock: emulated time on the harness, wall time on a bare DkSession
      await poll(dk.clock, loaded, (names) => want.every((id) => names.includes(id)), {
        withinMs: MODULES_LOADED_EMULATED_MS,
        intervalMs: 500,
        message: `modules ${want.join(' ')} not listed`,
        retry: true,
      });
    },
    { detail: modules.map((m) => m.id).join(' ') },
  );
}

/** One prepared module alone (a test that does not read the log). Pass `device` to put the services it needs on the card. */
export function installModule(
  dk: DkSession,
  id: string,
  artifact: string,
  options?: { device?: Device; session?: Pick<Session, 'status' | 'client'> },
): Promise<void> {
  return installModules(dk, [{ id, artifact }], options);
}
