import { Show, createResource } from 'solid-js';
import type { ModulePageProps } from '@digi/module-ui';
import { schema } from './gen/settings';

export default function Panel(props: ModulePageProps) {
  // oxlint-disable-next-line solid/reactivity -- the page reads once when it opens
  const boot = createResource(() => props.client.settings(schema).get('boot'))[0];
  return (
    <>
      <Show when={boot()?.ok && boot()}>{(read) => <p>Boot value now: {String(read().ok && read().value)}</p>}</Show>
      <p>Changes to Boot value take effect the next time the module starts.</p>
    </>
  );
}
