import { isCancelled, isNoRuntime, isTimeout } from '../modules/backend';
import { noteActivity } from './activity';
import type { TransferApp } from './app';
import { readTransfer } from './read-transfer';
import { targetLabel } from './target';
import { failureLine } from './wait-core';

/** A stopped job reports what it completed to the Log. */
export interface CancelledRun {
  cancelled: string;
}

const haltOn = (app: TransferApp, e: unknown): void => {
  app.state.setHalted(isTimeout(e) || isNoRuntime(e));
  app.state.setNoRuntime(isNoRuntime(e));
};

/** The one busy action. It is not made by the page: a job goes on when the page is left, and the old and the new page share it. */
export const runTransfer = async (app: TransferApp, what: string, f: () => Promise<void | CancelledRun>) => {
  const { target } = app.target;
  const { activeMode, beginJob, endJob, error, log, progress, setError, setHalted, setNoRuntime } = app.state;
  if (!beginJob(what, cancellable(what) ? () => cancelJob(app) : undefined)) return;
  const binding = app.target.bindSession();
  const current = () => {
    try {
      binding.check();
      return true;
    } catch {
      return false;
    }
  };
  const samples = activeMode() === 'samples';
  noteActivity();
  setError('');
  // after a cancel or a "no reply" the device is not asked again at once (that would wait the whole time a second time)
  let skipRead = false;
  // a read is not logged (the page reads by itself); every other run is, with its result
  const logRun = (failure?: unknown, cancelled?: string) => {
    if (what === 'Reading') return;
    const modules = activeMode() === 'modules';
    app.env.logRun({
      what,
      target: targetLabel(target()),
      lines: modules ? log() : (app.env.extras?.samples?.lines?.() ?? []),
      message: modules ? progress()?.message : undefined,
      ...(failure === undefined
        ? cancelled === undefined
          ? {}
          : { cancelled: true, counts: cancelled }
        : { error: failureLine(failure, targetLabel(target())), cancelled: isCancelled(failure) }),
    });
  };
  try {
    // a job that the user stopped is not a failure: no error line, the log says so, and both sides are read again
    const result = await f();
    if (samples && !current()) return;
    setHalted(false);
    setNoRuntime(false);
    logRun(undefined, result ? result.cancelled : undefined);
  } catch (e) {
    if (samples && !current()) return;
    skipRead = isCancelled(e) || isTimeout(e) || isNoRuntime(e);
    haltOn(app, e);
    setError(failureLine(e, targetLabel(target())));
    logRun(e);
  } finally {
    if (!skipRead && (!samples || current())) {
      app.state.beginReadback();
      try {
        await readTransfer(app);
      } catch (e) {
        if (!samples || current()) {
          haltOn(app, e);
          if (!error()) setError(failureLine(e, targetLabel(target())));
        }
      }
    }
    // The target may have changed while Transfer was unmounted and unable to request a read.
    const again = endJob();
    if (again || (samples && binding.target !== target() && activeMode() === 'samples'))
      void runTransfer(app, 'Reading', async () => {});
    app.env.extras?.onJobEnd?.();
  }
};

/** The jobs whose Cancel is safe: a file is atomic on the card (the store changes at END only, and a cut-off upload is ABORTed), so a cancel
 * leaves the files that were verified and none half done. Delete and Restart are short and have nothing to stop in the middle. */
export const cancellable = (what: string): boolean => what === 'Uploading' || what === 'Installing';

/** Stop the running job: no later file starts, and every request that waits on the session ends as cancelled. */
export const cancelJob = (app: TransferApp): void => {
  app.state.askStop();
  app.target.cancelPending();
};
