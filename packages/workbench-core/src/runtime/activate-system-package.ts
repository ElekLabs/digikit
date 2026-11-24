import { DkError, fmtVersion, hasCap, poll, type DkInfoFull, type DkSession } from '@digi/dk-session';
import { systemPackageReads, systemPackageStore } from './system-package';

export type SystemPackageAction = 'put' | 'verify' | 'retry' | 'restart' | 'wait_for_boot' | 'confirm_support';

export interface SystemPackagePlanInput {
  repair: boolean;
  moduleSupport: boolean;
  verifiedPackageOnCard: boolean;
}

export interface SystemPackage {
  runtime: number;
  bytes: Uint8Array;
  plan(input: SystemPackagePlanInput): Promise<readonly SystemPackageAction[]>;
}

type Identity = { device: string; imageId?: string | null };

/** Write and verify the build matching this target, then confirm the new boot answers package commands. */
export async function activateSystemPackage(
  session: Pick<
    DkSession,
    'clock' | 'probe' | 'info' | 'modules' | 'list' | 'verify' | 'put' | 'retry' | 'restart' | 'waitForBoot'
  >,
  pkg: SystemPackage,
  options: {
    identity?: Identity;
    repair?: boolean;
    check: () => void;
    signal?: AbortSignal;
    /** Capture independent restart evidence before sending the guest RESTART command. */
    beforeRestart?: () => Promise<() => Promise<void>>;
    progress?: (fraction: number | undefined, detail: string) => void;
  },
): Promise<boolean> {
  const { repair, check, signal, progress } = options;
  const checkTarget = () => {
    signal?.throwIfAborted();
    check();
  };
  const quick = signal ? { signal } : true;
  const pollBoot = <T>(read: () => Promise<T>, done: (value: T) => boolean, message: string) =>
    poll(session.clock, read, done, {
      withinMs: 90_000,
      intervalMs: session.clock.pollMs ?? 400,
      message,
      retry: (error) => {
        checkTarget();
        // Native MIDI IPC can reject with a string while the selected port reopens.
        return (
          typeof error === 'string' ||
          (error instanceof DkError && (error.kind === 'timeout' || error.kind === 'transport'))
        );
      },
    });
  const info = async () => {
    checkTarget();
    const current = await session.probe((q) => session.info(q), quick);
    checkTarget();
    return current;
  };
  const initial = await pollBoot(info, () => true, 'System package target');
  const identity = options.identity ?? { device: initial.port, imageId: initial.image_id };
  const confirm = (current: DkInfoFull) => {
    if (current.port !== identity.device || (identity.imageId !== undefined && current.image_id !== identity.imageId))
      throw new Error('The target firmware changed.');
    if (current.runtime !== pkg.runtime)
      throw new Error(
        `The system package requires runtime ${fmtVersion(pkg.runtime)}. The target reports ${fmtVersion(current.runtime)}.`,
      );
    if (!hasCap(current, 'MODULES')) throw new Error('This runtime does not support system package activation.');
  };
  confirm(initial);
  const moduleSupport = async (current: DkInfoFull) => {
    const reads = systemPackageReads(current);
    await reads.command('MODULES', () => session.probe((q) => session.modules(q), quick));
    checkTarget();
    return !reads.coreOnly;
  };
  const ready = await moduleSupport(initial);
  const plan = async (verifiedPackageOnCard: boolean) => {
    checkTarget();
    const actions = await pkg.plan({ repair: repair ?? false, moduleSupport: ready, verifiedPackageOnCard });
    checkTarget();
    return actions;
  };
  // The planner can refuse repair on a healthy target without consulting its card.
  if (repair && ready) await plan(false);

  progress?.(undefined, 'Checking system package');
  const store = systemPackageStore(session, pkg.bytes, { info: initial, check: checkTarget });
  const actions = await plan(await store.verifiedOnCard());
  let waitForRestart: (() => Promise<void>) | undefined;
  for (const action of actions) {
    checkTarget();
    if (action === 'put' || action === 'retry' || action === 'restart') {
      const current = await info();
      confirm(current);
      if (current.card_read_only) throw new Error('The device card store is read-only.');
      if (current.upload_open) throw new Error('Another upload is in progress on this target.');
      checkTarget();
    }
    switch (action) {
      case 'put':
        progress?.(0, 'Uploading system package');
        await store.put((sent, total) => {
          checkTarget();
          progress?.(sent / total, 'Uploading system package');
        });
        break;
      case 'verify':
        progress?.(undefined, 'Verifying system package');
        await store.verify();
        break;
      case 'retry':
        await session.retry();
        break;
      case 'restart':
        progress?.(undefined, 'Restarting for system package');
        if (options.beforeRestart) {
          waitForRestart = await options.beforeRestart();
          checkTarget();
        }
        await session.restart();
        break;
      case 'wait_for_boot':
        try {
          if (waitForRestart) await waitForRestart();
          else await session.waitForBoot(90_000, { signal, requireBootEvidence: true });
        } catch (error) {
          if (!(error instanceof DkError && error.kind === 'timeout')) throw error;
          throw new Error(
            `Could not confirm system package activation (${error.message}). Restart the device manually and check module support in Transfer.`,
            { cause: error },
          );
        }
        break;
      case 'confirm_support':
        progress?.(undefined, 'Confirming module support');
        await pollBoot(
          async () => {
            const current = await info();
            confirm(current);
            return moduleSupport(current);
          },
          (active) => active,
          'Module support after system package restart',
        );
        break;
    }
  }
  checkTarget();
  return actions.length > 0;
}
