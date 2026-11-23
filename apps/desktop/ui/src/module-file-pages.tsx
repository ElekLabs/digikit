import { Show, createSignal } from 'solid-js';
import { bindModuleFiles, type ModuleFileClient } from '@digi/workbench-core/module-ui/module-files.ts';
import type { ModulePage } from '@digi/workbench-core/module-ui/module-page.ts';
import { errorLine } from '@digi/workbench-core/modules/backend.ts';
import type { TransferApp } from '@digi/workbench-core/transfer/app.ts';
import type { ModuleFilesSpec } from '@digi/workbench-core/transfer/env.ts';
import { MODULE_FILES } from '@digi/workbench-core/transfer/module-files.generated.ts';
import ModuleFilesArea from '@digi/workbench-core/ui/transfer/ModuleFilesArea.tsx';
import type { PageCtx } from '@digi/workbench-core/ui/transfer/TransferParts.tsx';
import { cardRefresh } from './card-state';

function FilePanel(props: {
  id: string;
  spec: ModuleFilesSpec;
  app: TransferApp;
  files: ModuleFileClient;
  current: () => boolean;
}) {
  const [busy, setBusy] = createSignal('');
  const [error, setError] = createSignal('');
  const [reload, setReload] = createSignal(0);
  const ctx: PageCtx = {
    busy,
    setError,
    reload,
    async run(label, run) {
      if (busy() || !props.current()) return;
      setBusy(label);
      setError('');
      try {
        await props.app.jobs.runJob(`${label} ${props.spec.label.toLowerCase()}`, { group: 'target' }, run);
      } catch (e) {
        if (props.current()) setError(errorLine(e));
      } finally {
        if (props.current()) {
          setBusy('');
          setReload((value) => value + 1);
        }
      }
    },
  };
  return (
    <div class="transfer-page module-file-controls">
      <Show when={error()}>
        <p class="err" role="alert">
          {error()}
        </p>
      </Show>
      <ModuleFilesArea
        ctx={ctx}
        id={props.id}
        spec={props.spec}
        files={props.files}
        refresh={() => reload() + cardRefresh()}
      />
    </div>
  );
}

export function moduleFilePages(app: TransferApp) {
  const pages = new Map<string, ModulePage>();
  for (const [id, spec] of Object.entries(MODULE_FILES)) {
    pages.set(id, {
      descriptor: { id, name: spec.name, settings: [], operations: [] },
      open(current) {
        const binding = app.target.bindSession();
        const files = bindModuleFiles(id, binding, current);
        return { Panel: () => <FilePanel id={id} spec={spec} files={files} app={app} current={current} /> };
      },
    });
  }
  return pages;
}
