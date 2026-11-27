import type { AnalyzerHost, AnalyzerPcm } from '@digi/workbench-core/host/host.ts';

type Source = Parameters<AnalyzerHost['open']>[0];
export type AnalyzerStatus = { kind: 'pending' | 'waiting' | 'live' } | { kind: 'error'; message: string };

const READ_INTERVAL_MS = 40;
const NO_AUDIO_MS = 750;

export function createAnalyzerStream(
  host: AnalyzerHost,
  receive: (revision: number, pcm: AnalyzerPcm) => void,
  canReceive: () => boolean,
  report: (status: AnalyzerStatus) => void,
) {
  let request = 0;
  let wanted: Source | null = null;
  let active: { revision: number; id: number; lastPcm: number } | undefined;
  let running = false;
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function closeActive() {
    if (!active) return;
    const { id } = active;
    active = undefined;
    try {
      await host.close(id);
    } catch {
      // The source may already have disconnected.
    }
  }

  async function pump() {
    if (running) return;
    running = true;
    const revision = request;
    try {
      if (active && active.revision !== revision) await closeActive();
      if (revision !== request || wanted === null) return;
      if (!active) {
        active = { revision, id: await host.open(wanted), lastPcm: performance.now() };
      }
      if (revision !== request || !canReceive()) return;
      const pcm = await host.read(active.id);
      if (revision !== request) return;
      if (
        pcm &&
        Number.isFinite(pcm.rate) &&
        pcm.rate >= 1000 &&
        pcm.rate <= 384000 &&
        pcm.left.length &&
        pcm.right.length
      ) {
        active.lastPcm = performance.now();
        receive(revision, pcm);
        report({ kind: 'live' });
      } else if (performance.now() - active.lastPcm >= NO_AUDIO_MS) {
        report({ kind: 'waiting' });
      }
    } catch (error) {
      if (revision === request && wanted !== null) {
        wanted = null;
        report({ kind: 'error', message: error instanceof Error ? error.message : String(error) });
        await closeActive();
      }
    } finally {
      running = false;
      if (revision !== request) {
        void pump();
      } else if (wanted !== null) {
        timer = setTimeout(() => {
          timer = undefined;
          void pump();
        }, READ_INTERVAL_MS);
      }
    }
  }

  return {
    select(source: Source | null) {
      if (timer !== undefined) clearTimeout(timer);
      timer = undefined;
      wanted = source;
      request++;
      report({ kind: 'pending' });
      void pump();
      return request;
    },
  };
}
