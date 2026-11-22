// HEALTH samples from the selected device's runtime log.
import { runtimeEvent, type DkSession } from '@digi/dk-session';
import { CAP, LOG_KIND } from '@digi/dk-session/gen_proto';

export interface DeviceHealth {
  seq: number;
  blocks: number;
  cpu: number | null;
  dsp: number | null;
  peak: number | null;
}

export async function latestDeviceHealth(
  session: Pick<DkSession, 'logPage'>,
  signal?: AbortSignal,
): Promise<DeviceHealth | undefined> {
  const page = (from: number) => {
    signal?.throwIfAborted();
    return session.logPage(from, true);
  };
  const [oldest, newest, first] = await page(0);
  if (!first.length) return;
  // Walk the retained window newest first. The first reply supplies the actual page size.
  let end = newest + 1;
  while (end > oldest) {
    const from = Math.max(oldest, end - first.length);
    const events = from === oldest ? first : (await page(from))[2];
    const event = events.findLast((e) => e.seq < end && e.kind === LOG_KIND.HEALTH);
    if (event) {
      const { fields } = runtimeEvent(event);
      return {
        seq: event.seq,
        blocks: event.time,
        cpu: typeof fields.cpu_pct === 'number' ? fields.cpu_pct : null,
        dsp: typeof fields.dsp_avg_pct === 'number' ? fields.dsp_avg_pct : null,
        peak: typeof fields.dsp_max_pct === 'number' ? fields.dsp_max_pct : null,
      };
    }
    end = from;
  }
}

/** Let the current exchange finish when a job takes over. Firmware waits for that exchange before raw MIDI I/O. */
export async function readDeviceHealth(session: Pick<DkSession, 'info' | 'logPage'>, signal: AbortSignal) {
  signal.throwIfAborted();
  const info = await session.info(true);
  signal.throwIfAborted();
  return info.caps !== null && info.caps & (1 << CAP.LOG) ? latestDeviceHealth(session, signal) : undefined;
}

export const percent = (value: number | null | undefined) => (value == null ? '—' : Math.round(value) + '%');
