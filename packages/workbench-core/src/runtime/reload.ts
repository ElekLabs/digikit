// The result of `dev.reload` (docs/CONTROL.md): the workbench asks the daemon to build the stale ColdFire objects of the session's
// modules, apply the modules again from the objects on disk and bring the running session to them.

export interface ReloadBuild {
  /** Modules that had stale objects: the objects written and the milliseconds of the build. */
  built: { id: string; objects: string[]; ms: number }[];
  /** Modules whose objects were current. */
  skipped: string[];
  ms: number;
}

export interface ReloadReport {
  /** `swap`: UI module bytes written into a restored main-screen state. `boot`: the new image was loaded (a cold boot unless it was
   * booted before). `dsp`: only the DSP section changed (hot injection). `none`: the running image already is the new one. */
  path: 'swap' | 'boot' | 'dsp' | 'none';
  /** Why a boot was needed (`boot` and `dsp`). */
  reason?: string;
  /** Milliseconds of a `swap` or `none`. */
  ms?: number;
  /** Seconds of a `boot` or `dsp` (the report of `firmware.load`). */
  seconds?: number;
  ranges?: number;
  bytes?: number;
  owners?: string[];
  /** Present when the call asked for a build. */
  build?: ReloadBuild;
}

const wall = (report: ReloadReport) =>
  report.ms !== undefined
    ? `${Math.round(report.ms)} ms`
    : report.seconds !== undefined
      ? `${report.seconds.toFixed(1)} s`
      : '';

const builtNote = (report: ReloadReport): string => {
  const built = report.build?.built ?? [];
  return built.length ? `built ${built.map((b) => b.id).join(', ')} (${Math.round(report.build?.ms ?? 0)} ms); ` : '';
};

/** One line for the status area. */
export const reloadLine = (report: ReloadReport): string => {
  const time = wall(report);
  const built = builtNote(report);
  switch (report.path) {
    case 'none':
      return `Reload: ${built}nothing changed (${time})`;
    case 'swap':
      return `Reload: ${built}swapped ${report.owners?.join(', ') || 'no module'}, ${report.bytes ?? 0} bytes in ${report.ranges ?? 0} ranges (${time})`;
    case 'dsp':
      return `Reload: ${built}DSP only, injected (${time})`;
    default:
      return `Reload: ${built}booted the new image, ${report.reason ?? 'the core changed'} (${time})`;
  }
};

/** An error of `dev.reload` for the status area: the daemon's text, without the method prefix it adds. A build error has the
 * compiler output after its first line: `status` is the first line (short), `full` the whole text. */
export const reloadError = (error: unknown): { status: string; full: string } => {
  const text = (error instanceof Error ? error.message : String(error)).replace(/^dev\.reload:\s*/, '');
  const lines = text.split('\n');
  const first = lines[0].replace(/ \(code -?\d+\)$/, '');
  // the first compiler error, with the path cut to the file name
  const cause = lines
    .slice(1)
    .find((l) => /\berror\b/.test(l))
    ?.replace(/^\S*\//, '');
  return {
    status: `Reload failed: ${first}${cause ? ` ${cause}` : ''}`,
    full: lines.length > 1 ? `Reload failed: ${text}` : '',
  };
};
