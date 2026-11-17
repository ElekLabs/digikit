/** The status fields that show a restart or load making progress (the daemon's `status` reply has more). */
export interface ProgressStatus {
  job?: {
    kind?: 'boot' | 'card' | 'step' | 'run_until' | 'firmware.load';
    wall_seconds?: number;
    phase?: string;
    progress?: number;
    icount?: number;
  } | null;
  epoch?: unknown;
  cf?: object | null;
}

/** A job (cold boot, card load, stepping or `firmware.load`) is running: the emulated clock may stand still while it does. */
export function isLoading(status: ProgressStatus | undefined): boolean {
  return Boolean(status?.job);
}

/**
 * Movement other than the emulated clock, as a string that changes with it: the epoch and, only while a job runs,
 * the ColdFire and job instruction counts (a cold boot moves only these). Outside a job only the epoch counts,
 * so a halted DSP with a running ColdFire is still stuck.
 */
export function restartProgress(status: ProgressStatus | undefined): string {
  const loading = isLoading(status);
  return [status?.epoch, loading ? icount(status?.cf) : undefined, loading ? icount(status?.job) : undefined].join('|');
}

function icount(counts: object | null | undefined): unknown {
  return (counts as { icount?: unknown } | null | undefined)?.icount;
}

/** The counts for a stuck message, empty when no job runs. */
export function describeRestart(status: ProgressStatus | undefined): string {
  if (!isLoading(status)) return '';
  return ` (job active: icount ${icount(status?.cf)}, job ${JSON.stringify(status?.job)})`;
}
