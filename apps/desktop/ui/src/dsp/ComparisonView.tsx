import { For, Show, Suspense, createSignal, lazy, onCleanup, onMount } from 'solid-js';
import type { FsHost } from '@digi/workbench-core/host/host.ts';
import { SPECTRUM_FLOOR, type EnvelopeFrame, type EnvelopeTrace, type Stereo } from '../analyzer/frame';
import { parseComparisonEvidence, readQualityMedia, type QualityComparison } from './quality-report';
import ComparisonAudio from './ComparisonAudio';
import '../analyzer/analyzer-panel.css';

const AnalyzerPlot = lazy(() => import('../analyzer/AnalyzerPlot'));

interface ComparisonMedia {
  urls: Stereo<string | undefined>;
  envelope?: EnvelopeFrame;
  plot?: string;
  notices: string[];
}

function describeTrace(trace: EnvelopeTrace) {
  if (!trace.levels.length) return 'No complete RMS windows.';
  let min = Infinity;
  let max = -Infinity;
  for (const value of trace.levels) {
    min = Math.min(min, value);
    max = Math.max(max, value);
  }
  return `${trace.duration.toFixed(2)} s, ${min.toFixed(1)} to ${max.toFixed(1)} dBFS`;
}

export default function ComparisonView(props: {
  fs: Pick<FsHost, 'read'>;
  root: string;
  comparison: QualityComparison;
}) {
  const [media, setMedia] = createSignal<ComparisonMedia>();
  const [plotError, setPlotError] = createSignal(false);
  const [position, setPosition] = createSignal(0);
  const envelope = () => {
    const frame = media()?.envelope;
    if (frame?.traces.some((trace) => trace.levels.length)) return frame;
  };
  const urls: string[] = [];
  let disposed = false;

  onMount(async () => {
    const { comparison, fs, root } = props;
    const notices: string[] = [];
    async function load<T>(label: string, name: string | undefined, fetch: (name: string) => Promise<T>) {
      try {
        if (!name) throw new Error('No portable copy is listed under quality-media.');
        return await fetch(name);
      } catch (error) {
        notices.push(`${label} · ${error instanceof Error ? error.message : String(error)}`);
      }
    }
    const fallback = comparison.plots.find((file) => file.startsWith('quality-media/waveform-')) ?? comparison.plots[0];
    const [baseline, candidate, evidence, plot] = await Promise.all([
      load('Baseline WAV', comparison.baseline.wav, (name) => readQualityMedia(fs, root, name, 'wav')),
      load('Candidate WAV', comparison.candidate.wav, (name) => readQualityMedia(fs, root, name, 'wav')),
      load('Comparison evidence', comparison.file, async (name) =>
        parseComparisonEvidence(await readQualityMedia(fs, root, name, 'json')),
      ),
      fallback ? load('Saved plot', fallback, (name) => readQualityMedia(fs, root, name, 'png')) : undefined,
    ]);
    if (disposed) return;

    function objectUrl(bytes: ArrayBuffer | undefined, type: string) {
      if (!bytes) return;
      const url = URL.createObjectURL(new Blob([bytes], { type }));
      urls.push(url);
      return url;
    }
    setMedia({
      urls: [objectUrl(baseline, 'audio/wav'), objectUrl(candidate, 'audio/wav')],
      envelope: evidence?.envelope,
      plot: objectUrl(plot, 'image/png'),
      notices: [...(evidence?.notices ?? []), ...notices],
    });
  });

  onCleanup(() => {
    disposed = true;
    for (const url of urls) URL.revokeObjectURL(url);
  });

  return (
    <>
      <dl class="dsp-sources">
        <dt>A · Baseline</dt>
        <dd>{props.comparison.baseline.file}</dd>
        <dt>B · Candidate</dt>
        <dd>{props.comparison.candidate.file}</dd>
      </dl>
      <Show when={media()} fallback={<p role="status">Loading comparison media…</p>}>
        {(loaded) => (
          <>
            <For each={loaded().notices}>{(notice) => <p role="status">{notice}</p>}</For>
            <ComparisonAudio urls={loaded().urls} position={setPosition} />
            <Show when={envelope()} keyed>
              {(frame) => (
                <figure class="dsp-envelope">
                  <figcaption>RMS envelope · dBFS</figcaption>
                  <Show when={!plotError()} fallback={<p role="status">Interactive plot unavailable.</p>}>
                    <div class="analyzer-chart" aria-hidden="true">
                      <div class="analyzer-axis-y">
                        <span>0</span>
                        <span>{SPECTRUM_FLOOR / 2}</span>
                        <span>{SPECTRUM_FLOOR}</span>
                      </div>
                      <div class="analyzer-plot">
                        <Suspense>
                          <AnalyzerPlot
                            mode="Envelope"
                            frame={frame}
                            cursor={loaded().urls.some(Boolean) ? Math.min(position() / frame.duration, 1) : null}
                            unavailable={() => setPlotError(true)}
                          />
                        </Suspense>
                      </div>
                      <div class="analyzer-axis-x">
                        <span>0</span>
                        <span>{(frame.duration / 2).toFixed(2)}</span>
                        <span>{frame.duration.toFixed(2)} s</span>
                      </div>
                    </div>
                  </Show>
                  <ul class="dsp-legend">
                    <li>
                      <strong class="analyzer-left">A</strong> · {describeTrace(frame.traces[0])}
                    </li>
                    <li>
                      <strong class="analyzer-right">B</strong> · {describeTrace(frame.traces[1])}
                    </li>
                  </ul>
                </figure>
              )}
            </Show>
            <Show when={loaded().envelope && !envelope()}>
              <p>No complete RMS windows in this comparison.</p>
            </Show>
            <Show when={(plotError() || !envelope()) && loaded().plot}>
              {(url) => <img class="dsp-saved-plot" src={url()} alt="Saved comparison plot" />}
            </Show>
          </>
        )}
      </Show>
    </>
  );
}
