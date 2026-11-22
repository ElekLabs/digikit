import { fmtShare } from '@digi/workbench-core/profile/profile-core.ts';
import { For, Show } from 'solid-js';
import type { SpanInterval } from './span-intervals';
import './span-breakdown.css';

export default function SpanBreakdown(props: { interval?: SpanInterval }) {
  return (
    <div class="span-breakdown">
      <Show
        when={props.interval}
        fallback={<p class="span-breakdown-status">Hover over the plot to inspect recorded intervals.</p>}
      >
        {(interval) => (
          <Show when={interval().rows.length} fallback={<p class="span-breakdown-status">No recorded calls</p>}>
            <div class="span-breakdown-scroll" role="region" aria-label="Recorded spans" tabIndex={0}>
              <table class="span-breakdown-table" aria-label="Selected interval">
                <thead>
                  <tr>
                    <th scope="col">Owner</th>
                    <th scope="col" class="span-breakdown-number">
                      Interval share
                    </th>
                    <th scope="col" class="span-breakdown-number">
                      Calls
                    </th>
                  </tr>
                </thead>
                <tbody>
                  <For each={interval().rows}>
                    {(row) => (
                      <tr>
                        <th scope="row" title={row.id}>
                          {row.name}
                        </th>
                        <td class="span-breakdown-number">{fmtShare(row.share)}</td>
                        <td class="span-breakdown-number">{row.calls.toLocaleString('en-US')}</td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
          </Show>
        )}
      </Show>
    </div>
  );
}
