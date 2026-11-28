import { Show, createSignal, onCleanup } from 'solid-js';
import type { ModulePageProps } from '@digi/module-ui';
import { ProjectManager } from './gen/project_manager_pb.js';
import ProjectDetails from './ProjectDetails';
import ProjectList from './ProjectList';
import { projectFileName, readProjectFile } from './project-file';
import { slotNumber } from './project-text';
import './project-manager.css';

/** The firmware's projects: a list on the left, the selected project's details on the right. Read-only on the device; "Save selected" writes the ticked projects' `.dkp`
 * files to a folder on the computer. */
export default function Panel(props: ModulePageProps) {
  const [selected, setSelected] = createSignal<number>();
  const [checked, setChecked] = createSignal<ReadonlySet<number>>(new Set());
  const [saving, setSaving] = createSignal(false);
  const [saveMessage, setSaveMessage] = createSignal('');
  // oxlint-disable-next-line solid/reactivity -- the client belongs to the page and does not change while it is open
  const client = props.client;
  const service = client.service(ProjectManager);
  let open = true;
  onCleanup(() => {
    open = false;
  });

  // slot 0 is a project: test for undefined, not for a falsy slot
  const chosen = () => {
    const slot = selected();
    return slot === undefined ? undefined : { slot };
  };
  const check = (slot: number, on: boolean) =>
    setChecked((now) => {
      const next = new Set(now);
      if (on) next.add(slot);
      else next.delete(slot);
      return next;
    });

  /** Reads each ticked project's file in turn, then saves them all in one call. A project that could not be read is named and left out. */
  async function save(items: { slot: number; name: string }[]) {
    setSaving(true);
    setSaveMessage('Reading the projects…');
    try {
      const files = [];
      const failed: string[] = [];
      for (const item of items) {
        const read = await readProjectFile(service, item.slot, () => open);
        if (!open) return;
        if (read?.state === 'ok') files.push({ name: projectFileName(item.slot, read.file.name), bytes: read.file.bytes });
        else failed.push(slotNumber(item.slot));
      }
      let text = '';
      if (files.length) {
        setSaveMessage('Saving…');
        const result = await client.saveFiles(files);
        if (!open) return;
        text = result.cancelled
          ? 'Save cancelled.'
          : `Saved ${result.saved} ${result.saved === 1 ? 'file' : 'files'}${result.where ? ` to ${result.where}` : ''}.`;
      }
      if (failed.length) text = `${text} Could not read project ${failed.join(', ')}.`.trim();
      setSaveMessage(text);
    } catch (error) {
      if (open) setSaveMessage(error instanceof Error ? error.message : 'The save failed.');
    } finally {
      if (open) setSaving(false);
    }
  }

  return (
    <div class="transfer-page pm-page">
      <ProjectList
        client={client}
        selected={selected()}
        onSelect={setSelected}
        checked={checked()}
        onCheck={check}
        onSave={(items) => void save(items)}
        saving={saving()}
        message={saveMessage()}
      />
      <Show when={chosen()} fallback={<p class="empty pm-hint">Select a project to see its details.</p>}>
        {(picked) => <ProjectDetails client={client} slot={picked().slot} />}
      </Show>
    </div>
  );
}
