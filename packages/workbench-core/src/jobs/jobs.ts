// The running jobs of the app: one signal with the state of jobs-core.ts. One `JobsApp` per app (built by createTransferApp).
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

export class JobBusyError extends Error {}
export interface JobHandle {
  progress(fraction?: number, detail?: string): void;
}

export function createJobs() {
  const [jobs, setJobs] = createSignal(emptyJobs());
  const busy = (group: JobGroup): boolean => groupBusy(jobs(), group);
  /** The job shown in the status bar: the first one (an emulator job before a target job). */
  const currentJob = (): Job | undefined => {
    const list = jobs().list;
    return list.find((j) => j.group === 'emulator') ?? list[0];
  };
  /** Run `work` as a job of `group`; throws JobBusyError when the group has a job. The job ends when `work` settles. */
  async function runJob<T>(
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
  const syncControllerJob = (job: ControllerJob | null | undefined, cancel: () => void): void => {
    setJobs((s) => syncController(s, job, Date.now(), cancel));
  };
  return { jobs, setJobs, busy, currentJob, runJob, syncControllerJob };
}
export type JobsApp = ReturnType<typeof createJobs>;
