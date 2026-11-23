import { DefaultModuleError } from '@digi/workbench-core/firmware/default-modules-core.ts';
import type { FirmwareEnv } from '@digi/workbench-core/firmware/env.ts';
import { JobBusyError, type JobHandle, type JobsApp } from '@digi/workbench-core/jobs/jobs.ts';
import { isCancelled } from '@digi/workbench-core/modules/backend.ts';
import { failureEntry } from '../log-core';
import type { LogWriter } from '../log-state';

export type DefaultModules = Pick<NonNullable<FirmwareEnv['defaultModules']>, 'install'> & {
  tickedFor: (device: string) => string[];
};

type DefaultInstallOptions = {
  defaults: DefaultModules;
  runJob: JobsApp['runJob'];
  signal: AbortSignal;
  cancelControllerJob: () => void;
  controllerBusy: () => boolean;
  setError: (error: string) => void;
  setReloadNote: (note: string) => void;
  logInput: LogWriter['logInput'];
  bumpCardRefresh: () => void;
};

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** Install and enable the selected defaults on the emulator's card after its firmware boots. */
export function createDefaultInstall({
  defaults,
  runJob,
  signal,
  cancelControllerJob,
  controllerBusy,
  setError,
  setReloadNote,
  logInput,
  bumpCardRefresh,
}: DefaultInstallOptions) {
  const performDefaultInstall = async (deviceId: string, job: JobHandle, installSignal: AbortSignal) => {
    const ids = defaults.tickedFor(deviceId);
    if (ids.length === 0) return;
    try {
      const lines = await defaults.install(
        'emulator',
        deviceId,
        ids,
        (p) => {
          if (!signal.aborted) job.progress(undefined, p.text);
        },
        { signal: installSignal, stillBooting: controllerBusy },
      );
      if (signal.aborted) return;
      setReloadNote(`Default modules ready: ${ids.join(', ')}`);
      logInput({
        source: 'firmware',
        level: 'info',
        text: `default modules in the emulator: ${ids.join(', ')}`,
        detail: lines.join('\n'),
      });
    } catch (e) {
      if (signal.aborted) return;
      if (isCancelled(e) || isCancelled((e as { cause?: unknown })?.cause)) {
        logInput({ source: 'firmware', level: 'info', text: 'default modules cancelled' });
        bumpCardRefresh();
        return;
      }
      const what = e instanceof DefaultModuleError ? `default module ${e.id}` : 'default modules';
      setReloadNote(`${what} failed`);
      logInput(failureEntry('firmware', what, e instanceof DefaultModuleError ? e.message : message(e)));
    }
    bumpCardRefresh();
  };

  return async (deviceId: string): Promise<void> => {
    const stop = new AbortController();
    const abortInstall = () => stop.abort();
    signal.addEventListener('abort', abortInstall, { once: true });
    try {
      await runJob(
        'Installing default modules',
        {
          group: 'emulator',
          cancel: () => {
            stop.abort();
            cancelControllerJob();
          },
        },
        (job) => performDefaultInstall(deviceId, job, stop.signal),
      );
    } catch (e) {
      if (!signal.aborted && !(e instanceof JobBusyError)) setError(message(e));
    } finally {
      signal.removeEventListener('abort', abortInstall);
    }
  };
}
