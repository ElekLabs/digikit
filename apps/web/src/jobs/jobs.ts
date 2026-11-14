import { createSignal } from 'solid-js';
import {
  addJob,
  emptyJobs,
  groupBusy,
  removeJob,
  syncController,
  updateJob,
  type ControllerJob,
  type Job,
  type JobGroup,
} from './jobs-core';

export const [jobs, setJobs] = createSignal(emptyJobs());
export const busy = (group: JobGroup): boolean => groupBusy(jobs(), group);
/** The job shown in the status bar: the first one (an emulator job before a target job). */
export const currentJob = (): Job | undefined => {
  const list = jobs().list;
  return list.find((j) => j.group === 'emulator') ?? list[0];
};
export class JobBusyError extends Error {}
export interface JobHandle {
  progress(fraction?: number, detail?: string): void;
}
/** Run `work` as a job of `group`; throws JobBusyError when the group has a job. The job ends when `work` settles. */
export async function runJob<T>(
  label: string,
  opts: { group: JobGroup; cancel?: () => void },
  work: (job: JobHandle) => Promise<T>,
): Promise<T> {
  const added = addJob(jobs(), { group: opts.group, label, cancel: opts.cancel }, Date.now());
  if (!added) {
    const running = jobs().list.find((j) => j.group === opts.group);
    throw new JobBusyError(`${opts.group} is busy: ${running?.label ?? ''}`);
  }
  const id = added.id;
  setJobs(added.state);
  try {
    return await work({
      progress: (fraction, detail) => setJobs((s) => updateJob(s, id, { progress: fraction, detail })),
    });
  } finally {
    setJobs((s) => removeJob(s, id));
  }
}
export const syncControllerJob = (job: ControllerJob | null | undefined, cancel: () => void): void => {
  setJobs((s) => syncController(s, job, Date.now(), cancel));
};
