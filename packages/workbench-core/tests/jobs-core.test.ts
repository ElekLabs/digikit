import { describe, expect, it } from 'vitest';
import {
  addJob,
  emptyJobs,
  groupBusy,
  removeJob,
  syncController,
  controllerLabel,
  updateJob,
} from '../src/jobs/jobs-core.ts';

describe('jobs', () => {
  it('allows one job per group and frees the group on remove', () => {
    const first = addJob(emptyJobs(), { group: 'emulator', label: 'Boot' }, 1)!;
    expect(addJob(first.state, { group: 'emulator', label: 'Again' }, 2)).toBeUndefined();
    const other = addJob(first.state, { group: 'target', label: 'Upload' }, 2);
    expect(other).toBeDefined();
    const freed = removeJob(first.state, first.id);
    expect(groupBusy(freed, 'emulator')).toBe(false);
    expect(addJob(freed, { group: 'emulator', label: 'Again' }, 3)).toBeDefined();
  });
  it('an update that changes nothing returns the same state', () => {
    const first = addJob(emptyJobs(), { group: 'target', label: 'Upload', progress: 0.5, detail: '1/2' }, 1)!;
    expect(updateJob(first.state, first.id, { progress: 0.5, detail: '1/2' })).toBe(first.state);
    const changed = updateJob(first.state, first.id, { progress: 0.6 });
    expect(changed).not.toBe(first.state);
    expect(changed.list[0].progress).toBe(0.6);
  });
});

describe('syncController', () => {
  const cancel = () => {};
  it('copies progress and phase onto an own emulator job without an external job', () => {
    const own = addJob(emptyJobs(), { group: 'emulator', label: 'Load' }, 1)!;
    const next = syncController(own.state, { kind: 'boot', wall_seconds: 2, phase: 'dsp', progress: 0.5 }, 2, cancel);
    expect(next.list).toHaveLength(1);
    expect(next.list[0]).toMatchObject({ id: own.id, progress: 0.5, detail: 'dsp' });
    expect(next.list[0].external).toBeUndefined();
  });
  it('shows a controller job as an external job and updates it', () => {
    const job = { kind: 'card', wall_seconds: 1, phase: 'copy', progress: 0.1 };
    const a = syncController(emptyJobs(), job, 5, cancel);
    expect(a.list).toHaveLength(1);
    expect(a.list[0]).toMatchObject({ external: true, label: controllerLabel(job), progress: 0.1, cancel });
    const b = syncController(a, { ...job, progress: 0.7 }, 6, cancel);
    expect(b.list).toHaveLength(1);
    expect(b.list[0]).toMatchObject({ id: a.list[0].id, progress: 0.7 });
  });
  it('removes the external job when the controller has none and keeps an own job', () => {
    const ext = syncController(emptyJobs(), { kind: 'boot', wall_seconds: 1 }, 1, cancel);
    expect(syncController(ext, null, 2, cancel).list).toHaveLength(0);
    const own = addJob(emptyJobs(), { group: 'emulator', label: 'Load' }, 1)!;
    expect(syncController(own.state, null, 2, cancel).list).toHaveLength(1);
  });
});
