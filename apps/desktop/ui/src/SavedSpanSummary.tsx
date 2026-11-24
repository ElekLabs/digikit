import { For, Show } from 'solid-js';
import { formatSpanShare, type SpanSummary } from '@digi/workbench-core/profile/span-summary.ts';
import './span-breakdown.css';

const number = (value?: number) =>
  value === undefined ? '—' : value.toLocaleString('en-US', { maximumFractionDigits: 2 });

export default function SavedSpanSummary(props: { spans?: SpanSummary; label?: string }) {
  return (
    <section class="span-breakdown saved-span-summary" aria-label="Saved span summary">
      <p class="span-breakdown-status" title={props.label}>
        Saved spans · {props.label} · Main CPU totals for the recorded window
      </p>
      <Show when={props.spans} fallback={<p class="span-breakdown-status">This capture has no recorded spans.</p>}>
        {(spans) => (
          <div class="span-breakdown-scroll" role="region" aria-label="Saved spans" tabIndex={0}>
            <table class="span-breakdown-table">
              <thead>
                <tr>
                  <th scope="col">Owner</th>
                  <th scope="col" class="span-breakdown-number">
                    CPU share
                  </th>
                  <th scope="col" class="span-breakdown-number">
                    Calls
                  </th>
                  <th scope="col" class="span-breakdown-number">
                    Total µs
                  </th>
                  <th scope="col" class="span-breakdown-number">
                    Max µs
                  </th>
                </tr>
              </thead>
              <tbody>
                <For each={spans().rows}>
                  {(row) => (
                    <tr>
                      <th scope="row">{row.name}</th>
                      <td class="span-breakdown-number">{formatSpanShare(row.share)}</td>
                      <td class="span-breakdown-number">{number(row.calls)}</td>
                      <td class="span-breakdown-number">{number(row.totalUs)}</td>
                      <td class="span-breakdown-number">{number(row.maxUs)}</td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </div>
        )}
      </Show>
    </section>
  );
}
