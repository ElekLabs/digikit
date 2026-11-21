// The launcher reports host and DSP build states separately through `emu_build_info`.
export interface BuildInfo {
  pgo?: string | null;
  pgo_detail?: string | null;
  dsp_pgo?: Readonly<Record<string, 'ok' | 'plain' | 'external' | 'unavailable'>> | null;
}

const HOST_WARN = new Set(['stale', 'missing', 'failed', 'mismatch']);

export function pgoWarning(
  info: BuildInfo | undefined,
  device: string | undefined,
  dspEnabled = true,
): { label: string; title: string } | undefined {
  if (!info || !device || info.pgo === 'off' || info.pgo === 'skipped') return undefined;
  const hostWarning = info.pgo != null && HOST_WARN.has(info.pgo);
  const plainDsp = dspEnabled && info.dsp_pgo?.[device] === 'plain';
  if (!plainDsp && (info.dsp_pgo || !hostWarning)) return undefined;

  const lines: string[] = [];
  if (plainDsp)
    lines.push(
      'The DSP core for this firmware was built without a valid PGO profile.',
      `Run mise run perf:pgo:${device} and relaunch.`,
    );
  if (hostWarning) {
    lines.push(
      info.pgo === 'mismatch'
        ? 'The host PGO profile does not match this build.'
        : 'The emulator host was built without a valid PGO profile.',
    );
    const reason = info.pgo_detail?.trim();
    if (reason) lines.push(/[.!?]$/.test(reason) ? reason : `${reason}.`);
  }
  if (!plainDsp) lines.push('Run mise run perf:pgo:both and relaunch.');
  return { label: plainDsp ? 'no DSP PGO' : 'no host PGO', title: lines.join('\n') };
}
