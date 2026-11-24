import { Show, type JSX } from 'solid-js';
import HotspotTable from './HotspotTable';
import type { SavedProfile } from './hotspots';

export function OpenProfileButton(props: { open(file: File | undefined): Promise<void> }): JSX.Element {
  return (
    <label class="act quiet file">
      Open profile…
      <input
        type="file"
        accept=".json,application/json"
        onChange={(e) => {
          void props.open(e.currentTarget.files?.[0]);
          e.currentTarget.value = '';
        }}
      />
    </label>
  );
}

export function ReferenceView(props: {
  name?: string;
  pcs?: SavedProfile['pcs'];
  error: string;
  openReview?(path: string, sha256: string, entry: number): Promise<void>;
}): JSX.Element {
  return (
    <>
      <Show when={props.pcs} keyed>
        {(pcs) => (
          <Show when={pcs.cf.length || pcs.sharc.length}>
            <div class="prof-sec ruled">
              <div class="sec-head">
                <span class="title" title={props.name}>
                  Saved hotspots · {props.name}
                </span>
              </div>
              <Show when={pcs.cf.length}>
                <HotspotTable title="ColdFire PCs" rows={pcs.cf} openReview={props.openReview} />
              </Show>
              <Show when={pcs.sharc.length}>
                <HotspotTable title="SHARC PCs" rows={pcs.sharc} openReview={props.openReview} />
                <Show
                  when={
                    props.openReview &&
                    pcs.sharc.some((row) => row.owner === 'firmware') &&
                    !pcs.sharc.some((row) => row.review?.url)
                  }
                >
                  <div class="hint">
                    No catalog links for these hotspots. Generate the SHARC review for this firmware and refresh
                    Firmware symbols in Settings. Links appear for functions included in the review.
                  </div>
                </Show>
              </Show>
            </div>
          </Show>
        )}
      </Show>
      <Show when={props.error}>
        <p class="err" role="alert">
          {props.error}
        </p>
      </Show>
    </>
  );
}
