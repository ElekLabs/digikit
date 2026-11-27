import type { ModuleSettingsEndpoint } from '@digi/dk-session';
import type { DescribedSchema, DynamicSettings, FileNameSetting, ModuleFileClient } from '@digi/module-ui';
import { For, Show, createSignal } from 'solid-js';
import { deviceSettings } from '@digi/workbench-core/module-ui/device-settings.ts';
import { bindModuleFiles } from '@digi/workbench-core/module-ui/module-files.ts';
import type { ModulePage } from '@digi/workbench-core/module-ui/module-page.ts';
import { errorLine } from '@digi/workbench-core/modules/backend.ts';
import type { TransferApp } from '@digi/workbench-core/transfer/app.ts';
import type { ModuleFilesSpec } from '@digi/workbench-core/transfer/env.ts';
import { MODULE_FILES } from '@digi/workbench-core/transfer/module-files.generated.ts';
import FileChoice from '@digi/workbench-core/ui/module-ui/FileChoice.tsx';
import ModuleFilesArea from '@digi/workbench-core/ui/transfer/ModuleFilesArea.tsx';
import type { PageCtx } from '@digi/workbench-core/ui/transfer/TransferParts.tsx';
import { cardRefresh } from './card-state';

function FilePanel(props: {
  id: string;
  spec: ModuleFilesSpec;
  app: TransferApp;
  files: ModuleFileClient;
  current: () => boolean;
  settings?: DynamicSettings;
  choices: FileNameSetting[];
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
  const refresh = () => reload() + cardRefresh();
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
        refresh={refresh}
        settings={(files) => (
          <Show when={props.settings}>
            {(settings) => (
              <For each={props.choices}>
                {(field) => (
                  <FileChoice
                    ctx={ctx}
                    id={props.id}
                    field={field}
                    settings={settings()}
                    files={files}
                    refresh={refresh}
                  />
                )}
              </For>
            )}
          </Show>
        )}
      />
    </div>
  );
}

export function moduleFilePage(
  app: TransferApp,
  id: string,
  schema?: DescribedSchema,
  openSettings?: (current: () => boolean) => ModuleSettingsEndpoint,
): ModulePage {
  const choices = (schema?.settings ?? []).filter((field): field is FileNameSetting => field.kind === 'file-name');
  const spec = Object.hasOwn(MODULE_FILES, id)
    ? MODULE_FILES[id]
    : choices.length
      ? {
          name: schema!.name,
          label: 'Files',
          ext: [...new Set(choices.map((field) => field.extension))],
          preview: '',
        }
      : undefined;
  return {
    descriptor: {
      id,
      name: schema?.name ?? spec?.name ?? id,
      schema: schema ?? { version: 1, settings: [] },
    },
    open(current) {
      const binding = app.target.bindSession();
      const settings = schema && openSettings ? deviceSettings(schema, openSettings(current)) : undefined;
      const files = bindModuleFiles(id, binding, current);
      return {
        settings,
        Panel: spec
          ? () => (
              <FilePanel
                id={id}
                spec={spec}
                files={files}
                app={app}
                current={current}
                settings={settings}
                choices={choices}
              />
            )
          : undefined,
      };
    },
  };
}

export function moduleFilePages(app: TransferApp) {
  return new Map(Object.keys(MODULE_FILES).map((id) => [id, moduleFilePage(app, id)]));
}
