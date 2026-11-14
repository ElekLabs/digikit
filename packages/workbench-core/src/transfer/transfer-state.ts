// The Transfer page's state (Modules mode and the mode), kept for the life of the app: closing the page and opening it again shows the last list at once
// (it is refreshed behind it), and an install's verify results stay until the next restart. One `TransferState` per app (createTransferApp).
import { createSignal } from 'solid-js';
import type { JobsApp } from '../jobs/jobs';
import { addJob, removeJob, updateJob } from '../jobs/jobs-core';
import { remembered, type KeyValue } from '../storage';
import type { TransferEnv } from './env';
import type { LinkState } from './link-core';
import { parseMode, type Mode, type Progress, type TransferView, type Verified } from './transfer-core';

export interface DeviceInfo {
  name: string;
  runtime: string;
  free?: number;
  /** Modules mode: the storage line under the device line (storageLine); the free space is in it, not in the device line. */
  storage?: string;
  /** Samples mode only: the backend of the device's samples, and whether it makes folders. */
  samples?: string;
  canMkdir?: boolean;
}

/** The state of the Transfer page of one app. `jobs` is the app's (the busy action is a job of its store). */
export function createTransferState({
  env,
  storage,
  jobs,
}: {
  env: Pick<TransferEnv, 'extras'>;
  storage: KeyValue;
  jobs: JobsApp;
}) {
  const [lastView, setLastView] = createSignal<TransferView>();
  /** The library modules ticked for install. */
  const [ticks, setTicks] = createSignal<Set<string>>(new Set());
  /** The store paths ticked in the On device pane (for Delete). */
  const [delTicks, setDelTicks] = createSignal<Set<string>>(new Set());
  const [verified, setVerified] = createSignal<Verified>({});
  const [progress, setProgress] = createSignal<Progress>();
  const [showTests, setShowTests] = createSignal(false);
  const [log, setLog] = createSignal<string[]>([]);

  /** The mode of the page (Modules or Samples). */
  const [mode, setMode] = remembered(storage, 'digikit.transferMode', parseMode);
  /** The mode in use: Samples only where the app gives it (the desktop app). */
  const activeMode = (): Mode => (mode() === 'samples' && env.extras?.samples ? 'samples' : 'modules');
  /** What the On device column says about the device: its name (the profile's), the runtime, the free space of what the mode shows, and the
   * samples backend; `canMkdir` is known after the samples backend was opened. */
  const [deviceInfo, setDeviceInfo] = createSignal<DeviceInfo>();

  /** The link that carries the samples of the target (Samples mode, desktop app); undefined until the samples backend was opened for it. */
  const [sampleLink, setSampleLink] = createSignal<LinkState>();

  /** The device whose library the list shows while no device answers (the site). */
  const [browseDevice, setBrowseDevice] = remembered<'dt2' | 'dn2'>(storage, 'digikit.transferBrowseDevice', (v) =>
    v === 'dn2' ? 'dn2' : 'dt2',
  );
  /** The list shows the library alone, with no device behind it. */
  const [browsing, setBrowsing] = createSignal(false);
  /** Module ids from the page URL to tick once the list is read. */
  const [pendingAdd, setPendingAdd] = createSignal<string[]>([]);

  /** The module whose source the side panel shows (SourcePanel.tsx), or none. */
  const [sourceId, setSourceId] = createSignal<string>();

  /** The smoothed speed of the running transfer in bytes per second (the bottom bar sets it; the rows of the file being sent show it). */
  const [liveRate, setLiveRate] = createSignal<number>();

  /** The page's one busy action ('' when none) and its error line. They live here, not in the page: leaving the page unmounts it while a job
   * goes on, and the page that comes back must see the job (the buttons stay off, no second job, no read) and show its error. */
  const [reading, setReading] = createSignal(false);
  const busy = (): string => jobs.jobs().list.find((j) => j.group === 'target')?.label ?? (reading() ? 'Reading' : '');
  const [error, setError] = createSignal('');
  let readAgain = false;
  let stopAsked = false;
  let jobId: number | undefined;

  /** Start the one busy action `what`; false when another job runs (a read asked for then runs when that job ends: see `endJob`).
   * A write is a job of the shared store (group 'target'); a background read is not. */
  function beginJob(what: string, cancel?: () => void): boolean {
    if (busy()) {
      if (what === 'Reading') readAgain = true;
      return false;
    }
    if (what === 'Reading') setReading(true);
    else {
      const added = addJob(jobs.jobs(), { group: 'target', label: what, cancel }, Date.now());
      if (!added) return false;
      jobs.setJobs(added.state);
      jobId = added.id;
    }
    stopAsked = false;
    return true;
  }

  /** Report the progress of the running transfer job to the shared store (no-op with none). */
  const jobProgress = (fraction?: number, detail?: string): void => {
    const id = jobId;
    if (id !== undefined) jobs.setJobs((s) => updateJob(s, id, { progress: fraction, detail }));
  };

  /** The user asked to stop the running job (its Cancel button): a job that loops over files checks this before the next one. */
  const askStop = (): void => {
    stopAsked = true;
  };
  const stopWasAsked = (): boolean => stopAsked;

  /** End the busy action; true when a read was asked for during it. */
  function endJob(): boolean {
    setReading(false);
    if (jobId !== undefined) {
      const id = jobId;
      jobs.setJobs((s) => removeJob(s, id));
      jobId = undefined;
    }
    const again = readAgain;
    readAgain = false;
    return again;
  }

  /** After "no reply" or "no runtime" the page does not read by itself (TransferPage.tsx): Try again, another target, a change of the emulator's
   * runtime or a reconnect lets it read again. */
  const [halted, setHalted] = createSignal(false);
  const [noRuntime, setNoRuntime] = createSignal(false);

  return {
    lastView,
    setLastView,
    ticks,
    setTicks,
    delTicks,
    setDelTicks,
    verified,
    setVerified,
    progress,
    setProgress,
    showTests,
    setShowTests,
    log,
    setLog,
    mode,
    setMode,
    activeMode,
    deviceInfo,
    setDeviceInfo,
    sampleLink,
    setSampleLink,
    browseDevice,
    setBrowseDevice,
    browsing,
    setBrowsing,
    pendingAdd,
    setPendingAdd,
    sourceId,
    setSourceId,
    liveRate,
    setLiveRate,
    busy,
    error,
    setError,
    beginJob,
    jobProgress,
    askStop,
    stopWasAsked,
    endJob,
    halted,
    setHalted,
    noRuntime,
    setNoRuntime,
  };
}
export type TransferState = ReturnType<typeof createTransferState>;
