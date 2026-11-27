import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, untrack, type JSX } from 'solid-js';
import { formatBytes } from '@digi/workbench-core/modules/backend.ts';
import { useTransferApp } from '@digi/workbench-core/transfer/app-context.ts';
import { createPaneSelect } from '@digi/workbench-core/transfer/pane-select.ts';
import { parentOf } from '@digi/workbench-core/transfer/sample-upload.ts';
import { confirmDeleteLabel, crumbs, deleteRows } from '@digi/workbench-core/transfer/samples-core.ts';
import {
  Crumbs,
  DevicePane,
  DeleteButton,
  ListRow,
  SelectAll,
  UpRow,
  createConfirm,
  writing,
  type DeviceDrop,
  type PageCtx,
} from '@digi/workbench-core/ui/transfer/TransferParts.tsx';
import WaitNotice from '@digi/workbench-core/ui/transfer/WaitNotice.tsx';
import type { SamplesApp } from './app';

interface Props {
  ctx: PageCtx;
  destination: Pick<
    SamplesApp['destination'],
    'folder' | 'entries' | 'rows' | 'selected' | 'select' | 'note' | 'deleting'
  >;
  onDrop: DeviceDrop['onDrop'];
  open: SamplesApp['openDestination'];
  createFolder: SamplesApp['createFolder'];
  removeSelected: SamplesApp['removeSelected'];
  restart: SamplesApp['restart'];
}

export default function TargetSamplesPane(props: Props): JSX.Element {
  const app = useTransferApp();
  const { askStop, deviceInfo, readingAfterJob } = app.state;
  const { target, switching } = app.target;
  const available = () => !!deviceInfo() && target().kind !== 'none' && !switching();
  const { busy } = untrack(() => props.ctx);
  const unavailableMessage = () => {
    if (switching()) return 'Switching target…';
    if (target().kind !== 'none' && busy() === 'Reading') return 'Reading…';
    return 'No target available.';
  };
  const destination = untrack(() => props.destination);
  const {
    folder: sampleFolder,
    entries: sampleEntries,
    selected: sampleTicks,
    select: setSampleTicks,
    note,
    deleting,
  } = destination;
  const [naming, setNaming] = createSignal(false);
  const [newName, setNewName] = createSignal('');
  const confirm = createConfirm();
  onCleanup(confirm.stop);
  createEffect(
    on(
      [target, available],
      () => {
        confirm.stop();
        setNaming(false);
        setNewName('');
      },
      { defer: true },
    ),
  );

  const open = (folder: string) => {
    if (!available() || writing(busy())) return;
    confirm.stop();
    props.open(folder);
  };
  const createFolder = () => {
    if (!props.createFolder(newName().trim())) return;
    setNaming(false);
    setNewName('');
  };
  const rows = createMemo(destination.rows);
  const rowsByPath = createMemo(() => new Map(rows().map((r) => [r.path, r])));
  const nDelete = () => deleteRows(rows(), sampleTicks()).length;
  const dsel = createPaneSelect({
    keys: () =>
      rows()
        .filter((r) => !r.pending)
        .map((r) => r.path),
    selected: sampleTicks,
    set: (next) => {
      setSampleTicks(next);
      confirm.stop();
    },
    disabled: () => !available() || writing(busy()),
    open: (key) => {
      const r = rows().find((x) => x.path === key);
      if (r?.kind === 'folder' && !r.pending) open(r.path);
    },
    up: () => sampleFolder() && open(parentOf(sampleFolder())),
  });

  return (
    <DevicePane
      ctx={props.ctx}
      select={dsel}
      drop={{ onDrop: props.onDrop, disabled: () => !available() || writing(busy()) }}
      selectAll={
        <Show when={available()}>
          <SelectAll
            label="Select all to delete"
            keys={rows()
              .filter((r) => !r.pending)
              .map((r) => r.path)}
            ticked={sampleTicks()}
            disabled={writing(busy())}
            onChange={(next) => {
              setSampleTicks(next);
              confirm.stop();
            }}
          />
        </Show>
      }
      actions={
        <Show when={available()}>
          <Show when={deviceInfo()?.canMkdir}>
            <button type="button" class="act" disabled={!!busy()} onClick={() => setNaming(!naming())}>
              New folder
            </button>
          </Show>
          <DeleteButton
            n={nDelete()}
            busy={busy()}
            confirming={confirm.confirming()}
            confirmLabel={confirmDeleteLabel(deleteRows(rows(), sampleTicks()))}
            title="Delete the selected files and folders, with everything in them, from the device"
            onClick={() => confirm.click(() => void untrack(() => props.removeSelected()))}
          />
        </Show>
      }
    >
      <WaitNotice active={!!busy() && !switching()} cancel={busy() !== 'Uploading' && !readingAfterJob()} />
      <Show when={available()} fallback={<p class="empty">{unavailableMessage()}</p>}>
        <Show when={deviceInfo()?.apply === 'restart'}>
          <p class="empty note" role="status">
            <span>
              The device reads its samples when it starts. Added and deleted samples are used after a restart.
            </span>{' '}
            {/* The button needs the runtime. A stock device gets a text: the user restarts it by hand. */}
            <Show
              when={deviceInfo()?.hasRuntime}
              fallback={<span>Restart the {deviceInfo()?.name} to play new samples.</span>}
            >
              <button type="button" class="act small" disabled={!!busy()} onClick={() => void props.restart()}>
                Restart device
              </button>
            </Show>
          </p>
        </Show>
        <Crumbs
          items={crumbs(sampleFolder())}
          current={sampleFolder()}
          disabled={writing(busy())}
          label="Folder on the device"
          onOpen={open}
        />
        <Show when={deleting()}>
          <p class="empty note" role="status">
            <span>{deleting()}</span>{' '}
            <button type="button" class="act small" onClick={askStop}>
              Cancel
            </button>
          </p>
        </Show>
        <Show when={sampleFolder()}>
          <UpRow
            disabled={writing(busy())}
            title="Go up one folder (Backspace); a drop here goes into that folder"
            dropPath={parentOf(sampleFolder())}
            onUp={() => open(parentOf(sampleFolder()))}
          />
        </Show>
        <Show when={naming()}>
          <form
            class="newfolder"
            onSubmit={(e) => {
              e.preventDefault();
              createFolder();
            }}
          >
            <input
              type="text"
              aria-label="New folder name"
              placeholder="folder name"
              value={newName()}
              onInput={(e) => setNewName(e.currentTarget.value)}
            />
            <button type="submit" class="act small" disabled={!!busy()}>
              Create
            </button>
          </form>
        </Show>
        <Show when={sampleEntries()} fallback={<p class="empty">{busy() ? `${busy()}…` : 'No listing yet.'}</p>}>
          <Show when={rows().length} fallback={<p class="empty">This folder is empty.</p>}>
            <ul class="rows">
              <For each={rows().map((r) => r.path)}>
                {(path) => {
                  // Keep the DOM row and its checking timer across progress objects. Retain its last row during removal.
                  const initial = rowsByPath().get(path)!;
                  const r = () => rowsByPath().get(path) ?? initial;
                  return (
                    <ListRow
                      rowKey={r().pending ? undefined : r().path}
                      selected={sampleTicks().has(r().path)}
                      onSelect={r().pending ? undefined : (e) => dsel.click(r().path, e)}
                      check={{
                        label: `Delete ${r().name}`,
                        checked: sampleTicks().has(r().path),
                        disabled: writing(busy()) || r().pending,
                        onChange: () => dsel.tick(r().path),
                      }}
                      name={r().slot !== undefined ? `${r().slot}  ${r().name}` : r().name}
                      title={r().path}
                      folder={
                        r().kind === 'folder'
                          ? { disabled: writing(busy()) || r().pending, onOpen: () => open(r().path) }
                          : undefined
                      }
                      dropPath={r().kind === 'folder' && !r().pending ? r().path : undefined}
                      size={r().kind === 'file' ? formatBytes(r().size) : ''}
                      status={r().status}
                    />
                  );
                }}
              </For>
            </ul>
          </Show>
        </Show>
        <Show when={note()}>
          <p class="empty note">{note()}</p>
        </Show>
      </Show>
    </DevicePane>
  );
}
