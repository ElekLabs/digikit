import { For, Show, onMount, untrack, type JSX } from 'solid-js';
import { errorLine, formatBytes } from '@digi/workbench-core/modules/backend.ts';
import { createPaneSelect } from '@digi/workbench-core/transfer/pane-select.ts';
import { parentOf } from '@digi/workbench-core/transfer/sample-upload.ts';
import { foldersFromOutside, localCrumbs, localRowTags } from '@digi/workbench-core/transfer/samples-core.ts';
import { countLabel } from '@digi/workbench-core/transfer/transfer-core.ts';
import {
  Crumbs,
  ListRow,
  LocalPane,
  SelectAll,
  UpRow,
  writing,
  type PageCtx,
} from '@digi/workbench-core/ui/transfer/TransferParts.tsx';
import { useTransferApp } from '@digi/workbench-core/transfer/app-context.ts';
import type { SamplesApp } from './app';

interface Props {
  ctx: Pick<PageCtx, 'busy' | 'setError'>;
  source: Pick<
    SamplesApp['source'],
    'folder' | 'path' | 'selected' | 'select' | 'rows' | 'picked' | 'read' | 'choose' | 'open' | 'add'
  >;
  upload: () => Promise<void>;
}

export default function LocalSamplesPane(props: Props): JSX.Element {
  const { state, target } = useTransferApp();
  const canUpload = () => !!state.deviceInfo() && target.target().kind !== 'none' && !target.switching();
  const { busy, setError } = untrack(() => props.ctx);
  const source = untrack(() => props.source);
  const {
    folder: localFolder,
    path: localPath,
    selected: localTicks,
    select: setLocalTicks,
    rows: lrows,
    picked,
  } = source;
  let input: HTMLInputElement | undefined;
  const showError = (e: unknown) => setError(errorLine(e));
  onMount(() => void source.read().catch(showError));
  const choose = () => source.choose().catch(showError);
  const openLocal = (path: string) => {
    if (!writing(busy())) void source.open(path).catch(showError);
  };
  const addFiles = (files: File[], folders = 0) => {
    if (folders) setError(foldersFromOutside(folders));
    source.add(files);
  };
  const lsel = createPaneSelect({
    keys: () => lrows().map((r) => r.key),
    selected: localTicks,
    set: setLocalTicks,
    disabled: () => writing(busy()),
    open: (key) => {
      const row = lrows().find((r) => r.key === key);
      if (row?.kind === 'folder') openLocal(row.path);
    },
    up: () => localPath() && openLocal(parentOf(localPath())),
  });

  return (
    <LocalPane
      onDrop={addFiles}
      select={lsel}
      selectAll={
        <SelectAll
          label="Select all to upload"
          keys={lrows().map((r) => r.key)}
          ticked={localTicks()}
          disabled={writing(busy())}
          onChange={setLocalTicks}
        />
      }
      actions={
        <>
          <input
            ref={input}
            type="file"
            multiple
            hidden
            onChange={(e) => {
              const files = Array.from(e.currentTarget.files ?? []);
              e.currentTarget.value = '';
              addFiles(files);
            }}
          />
          <button
            type="button"
            class="act small"
            disabled={writing(busy())}
            title="Choose the folder on this computer that holds your samples"
            onClick={() => void choose()}
          >
            Choose folder…
          </button>
          <button
            type="button"
            class="act small"
            disabled={writing(busy())}
            title="Add files to the list, from any folder"
            onClick={() => input?.click()}
          >
            Add files…
          </button>
          <span class="drop">or drop files</span>
          <button
            type="button"
            class="act primary"
            disabled={!!busy() || picked().length === 0 || !canUpload()}
            title={
              picked().length
                ? 'Convert, upload and verify the selected files and folders into the open folder of the device'
                : 'Select a file or a folder to upload'
            }
            onClick={() => void props.upload()}
          >
            {busy() === 'Uploading' ? 'Uploading…' : countLabel('Upload', picked().length)}
          </button>
        </>
      }
    >
      <Show when={localFolder()}>
        <Crumbs
          items={localCrumbs(localFolder(), localPath())}
          current={localPath()}
          disabled={writing(busy())}
          label="Folder on your computer"
          onOpen={openLocal}
        />
      </Show>
      <Show when={localFolder() && localPath()}>
        <UpRow
          disabled={writing(busy())}
          title="Go up one folder (Backspace)"
          onUp={() => openLocal(parentOf(localPath()))}
        />
      </Show>
      <Show
        when={lrows().length}
        fallback={
          <p class="empty">
            {localFolder() ? 'Nothing here.' : 'Choose a folder, or add files. A WAV is converted when it is uploaded.'}
          </p>
        }
      >
        <ul class="rows">
          <For each={lrows()}>
            {(r) => (
              <ListRow
                rowKey={r.key}
                selected={localTicks().has(r.key)}
                onSelect={(e) => lsel.click(r.key, e)}
                check={{
                  label: `Upload ${r.name}`,
                  checked: localTicks().has(r.key),
                  disabled: writing(busy()),
                  onChange: () => lsel.tick(r.key),
                }}
                name={r.name}
                title={r.added ? `${r.name} (added)` : r.path}
                folder={
                  r.kind === 'folder' ? { disabled: writing(busy()), onOpen: () => openLocal(r.path) } : undefined
                }
                drag={writing(busy()) ? undefined : (e) => lsel.dragStart(r.key, e)}
                tags={localRowTags(r)}
                size={r.kind === 'file' ? formatBytes(r.size) : ''}
              />
            )}
          </For>
        </ul>
      </Show>
    </LocalPane>
  );
}
