import { Show, createResource } from 'solid-js';
import type { QueryError } from '@digi/module-ui';
import type { ModulePageProps } from '@digi/module-ui';
import { schema } from './gen/settings';
import { SettingsA } from './gen/settings_a_pb.js';

const reason = (error: QueryError) =>
  error.kind === 'busy' || error.kind === 'timeout'
    ? 'The device did not answer. Open the page again.'
    : error.kind === 'unavailable'
      ? 'The module is not available.'
      : `The module could not answer (${error.kind}).`;

export default function Panel(props: ModulePageProps) {
  // oxlint-disable-next-line solid/reactivity -- the page reads once when it opens
  const boot = createResource(() => props.client.settings(schema).get('boot'))[0];
  // oxlint-disable-next-line solid/reactivity -- the page asks once when it opens
  const atInit = createResource(() => props.client.service(SettingsA).bootAtInit({}))[0];
  return (
    <>
      <Show when={boot()?.ok && boot()}>{(read) => <p>Boot value now: {read().ok && read().unset ? 'not set' : String(read().ok && read().value)}</p>}</Show>
      <Show when={atInit()}>
        {(result) => (
          <p>
            Boot value at init:{' '}
            {(() => {
              const r = result();
              return r.ok ? (r.value.present ? r.value.value : 'not set') : reason(r.error);
            })()}
          </p>
        )}
      </Show>
      <p>Changes to Boot value take effect the next time the module starts.</p>
    </>
  );
}
