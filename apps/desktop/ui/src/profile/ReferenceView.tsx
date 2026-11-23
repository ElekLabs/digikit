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

export function ReferenceView(props: { name?: string; pcs?: SavedProfile['pcs']; error: string }): JSX.Element {
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
                <HotspotTable title="ColdFire PCs" rows={pcs.cf} />
              </Show>
              <Show when={pcs.sharc.length}>
                <HotspotTable title="SHARC PCs" rows={pcs.sharc} />
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
