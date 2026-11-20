// The Remote page's logic, without the page: what the state of digikit/remote says to the user, the device behind a session, and the
// runtime object the faceplate presses keys on. Pure, so host tests can run it (apps/desktop/ui/tests/remote-page.test.ts).
import { DkError, type DkSession, type RemoteState } from '@digi/dk-session';
import { writing } from '@digi/workbench-core/transfer/transfer-core.ts';
import { isNoRuntime } from '@digi/workbench-core/modules/backend.ts';
import { failureLine } from '@digi/workbench-core/transfer/wait-core.ts';
import { isDeviceId, type DeviceId } from '@digi/workbench-core/data/device-profiles.ts';

/**
 * The message under the faceplate for the state of digikit/remote (or for the error that asking for it gave). `listed` tells whether MODULES lists
 * digikit/remote (`undefined`: the list could not be read). The state request is optional in the module: E_PROTO with the module listed is no
 * "not loaded".
 */
export function remoteNotice(state: RemoteState | undefined, error?: unknown, listed?: boolean): string {
  if (isNoRuntime(error)) return (error as Error).message;
  if (error instanceof DkError) {
    if (error.status === 'E_PROTO') {
      if (listed === true) return '';
      return 'digikit/remote is not loaded on this device. Install it on the Transfer page, select it in MODULES on the device, apply and restart.';
    }
    return error.message;
  }
  if (error) return String(error);
  if (!state) return 'Reading the state of digikit/remote…';
  if (!state.enabled)
    return 'digikit/remote reports itself off. On the device, check it in SETTINGS > DIGIKIT > MODULES, apply and restart.';
  return '';
}

/** The module id the Remote page needs on the device. */
export const REMOTE_MODULE = 'digikit/remote';

/** The device answered REMOTE_STATE with E_PROTO: digikit/remote is not loaded (the runtime logs every such request as an error). */
export const isRemoteMissing = (error: unknown): boolean => error instanceof DkError && error.status === 'E_PROTO';

/**
 * Whether the page may ask REMOTE_STATE now. While the module is known to be missing it asks only when the loaded modules (MODULES, a
 * command every runtime answers without logging an error) list it. `loaded` is undefined when the list could not be read: then it asks.
 */
export const remoteStateWanted = (missing: boolean, loaded: readonly { name: string }[] | undefined): boolean =>
  !missing || !loaded || loaded.some((m) => m.name === REMOTE_MODULE);

/** Does MODULES list digikit/remote? `undefined` when the list could not be read. */
export const remoteListed = (loaded: readonly { name: string }[] | undefined): boolean | undefined =>
  loaded ? loaded.some((m) => m.name === REMOTE_MODULE) : undefined;

/** The lasting line for a failed key or turn: without the module (E_PROTO) it says what to do; anything else is the error's own text. */
export function keyErrorLine(e: unknown, target: string): string {
  if (isRemoteMissing(e))
    return 'Keys need digikit/remote: install it on the Transfer page, select it in MODULES, apply and restart.';
  return failureLine(e, target);
}

/** Where a key or turn reports its result: the page shows a failure until a key succeeds or the target changes. */
export interface KeyReport {
  ok: () => void;
  failed: (e: unknown) => void;
}

/** What the faceplate calls to press, release and turn: each becomes one request of the remote module. A failure is reported, then thrown as before. */
export function remoteRuntime(get: () => Promise<DkSession>, report?: KeyReport) {
  const run = async (request: (s: DkSession) => Promise<unknown>) => {
    try {
      await request(await get());
    } catch (e) {
      report?.failed(e);
      throw e;
    }
    report?.ok();
  };
  return {
    press: (code: number) => run((s) => s.remoteKey(code, true)),
    release: (code: number) => run((s) => s.remoteKey(code, false)),
    turn: (encoder: number, detents: number) => run((s) => s.remoteTurn(encoder, detents)),
  };
}

/** The device id from the INFO port name of a session (the id, or the id and a version: `<id>-1.17`), or undefined when it is not a known device. */
export function deviceOfPort(port: string): DeviceId | undefined {
  const d = port.split('-')[0] ?? '';
  return isDeviceId(d) ? d : undefined;
}

/** The Remote page asks INFO (it only names the device) at most this often while the target stays the same. */
export const INFO_EVERY_MS = 30_000;

/** Is INFO due? When it was never asked (`lastAt` undefined), the target changed, or `INFO_EVERY_MS` have passed. */
export const infoDue = (lastAt: number | undefined, now: number, targetChanged: boolean): boolean =>
  lastAt === undefined || targetChanged || now - lastAt >= INFO_EVERY_MS;

/**
 * The Remote page's polls wait while a Transfer job writes to the device: they would only queue behind it on the shared session (and slow it).
 * `busy` is the Transfer page's busy action ('' when none).
 */
export const remotePaused = (busy: string): boolean => writing(busy);

/**
 * Leaving the Remote page gives up the requests that wait on the session so the next page does not queue behind a silent device. While a
 * Transfer job runs, that session is the job's: cancelling would fail its requests and end an upload that the user did not stop.
 */
export const leaveCancels = (busy: string): boolean => !writing(busy);
