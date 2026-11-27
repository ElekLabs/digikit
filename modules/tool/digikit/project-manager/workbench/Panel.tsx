import { Show, createSignal } from 'solid-js';
import type { ModulePageProps } from '@digi/module-ui';
import ProjectDetails from './ProjectDetails';
import ProjectList from './ProjectList';
import './project-manager.css';

/** The firmware's projects: a list on the left, the selected project's details on the right. Read-only. */
export default function Panel(props: ModulePageProps) {
  const [selected, setSelected] = createSignal<number>();
  // slot 0 is a project: test for undefined, not for a falsy slot
  const chosen = () => {
    const slot = selected();
    return slot === undefined ? undefined : { slot };
  };
  return (
    <div class="pm-page">
      <ProjectList client={props.client} selected={selected()} onSelect={setSelected} />
      <Show when={chosen()} fallback={<p class="pm-hint">Select a project to see its details.</p>}>
        {(picked) => <ProjectDetails client={props.client} slot={picked().slot} />}
      </Show>
    </div>
  );
}
