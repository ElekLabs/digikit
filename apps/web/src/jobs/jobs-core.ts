// The running jobs of the workbench: one per group at a time. Pure state; jobs.ts holds the signal.
export type JobGroup = 'emulator' | 'target';
/** The controller's `status.job` (docs/CONTROL.md). */
export interface ControllerJob {
  kind: string;
  wall_seconds: number;
  phase?: string | null;
  progress?: number | null;
  icount?: number;
}
export interface Job {
  id: number;
  group: JobGroup;
  label: string;
  /** Date.now() at start. */
  started: number;
  /** 0..1, an estimate; undefined: unknown. */
  progress?: number;
  /** A short line under the label (a phase, "3/12"). */
  detail?: string;
  cancel?: () => void;
  /** Started by the controller, not by this page (a device RESTART's cold boot). */
  external?: boolean;
}
export interface JobsState {
  next: number;
  list: Job[];
}
export const emptyJobs = (): JobsState => ({ next: 1, list: [] });
export const groupBusy = (s: JobsState, group: JobGroup): boolean => s.list.some((j) => j.group === group);

/** Add a job; undefined when its group already has one. */
export function addJob(
  s: JobsState,
  job: Omit<Job, 'id' | 'started'>,
  now: number,
): { state: JobsState; id: number } | undefined {
  if (groupBusy(s, job.group)) return undefined;
  const id = s.next;
  return { state: { next: id + 1, list: [...s.list, { ...job, id, started: now }] }, id };
}

export function updateJob(
  s: JobsState,
  id: number,
  patch: Pick<Partial<Job>, 'progress' | 'detail' | 'label'>,
): JobsState {
  return { ...s, list: s.list.map((j) => (j.id === id ? { ...j, ...patch } : j)) };
}

export function removeJob(s: JobsState, id: number): JobsState {
  return { ...s, list: s.list.filter((j) => j.id !== id) };
}

/** The label of a controller job the page did not start. */
export function controllerLabel(job: ControllerJob): string {
  switch (job.kind) {
    case 'boot':
    case 'firmware.load':
      return 'Booting firmware';
    case 'card':
      return 'Loading the card';
    case 'step':
    case 'run_until':
      return 'Running the emulator';
    default:
      return job.kind;
  }
}

/** Mirror the controller's job (status poll, once a second): an emulator-group job of the page gets the controller's progress and phase;
 * with none, the controller job shows as an external job (cancel: `cancel`); with no controller job, the external job goes. */
export function syncController(
  s: JobsState,
  job: ControllerJob | null | undefined,
  now: number,
  cancel: () => void,
): JobsState {
  const own = s.list.find((j) => j.group === 'emulator' && !j.external);
  const ext = s.list.find((j) => j.external);
  if (!job) return ext ? removeJob(s, ext.id) : s;
  const progress = job.progress ?? undefined,
    detail = job.phase ?? undefined;
  if (own) {
    const next = updateJob(s, own.id, { progress, detail });
    return ext ? removeJob(next, ext.id) : next;
  }
  if (ext) return updateJob(s, ext.id, { label: controllerLabel(job), progress, detail });
  const id = s.next;
  const added: Job = {
    id,
    group: 'emulator',
    label: controllerLabel(job),
    started: now,
    progress,
    detail,
    cancel,
    external: true,
  };
  return { next: id + 1, list: [...s.list, added] };
}
