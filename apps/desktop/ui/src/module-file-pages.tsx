import {
  createModuleClient,
  type DescribedSchema,
  type DynamicSettings,
  type FileNameSetting,
  type ModuleChannel,
  type ModuleFileClient,
  type ModulePageDefinition,
} from '@digi/module-ui';
import { For, Show, createSignal } from 'solid-js';
import { Dynamic } from 'solid-js/web';
import { createDeviceModuleChannel } from '@digi/workbench-core/module-ui/device-module-channel.ts';
import { deviceSettingsChannel } from '@digi/workbench-core/module-ui/device-settings.ts';
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

/** What a module's page is made of: its settings schema and channel (absent for a module with a file panel only), the page it ships (absent: the generic form). */
export interface ModulePageParts {
  schema?: DescribedSchema;
  /** The channel of this page's mount; `current` ends with the mount and the instance. */
  channel: (current: () => boolean) => ModuleChannel;
  definition?: ModulePageDefinition;
}

export function moduleFilePage(app: TransferApp, id: string, parts: ModulePageParts): ModulePage {
  const { schema, channel, definition } = parts;
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
      hasPage: definition !== undefined,
      hasFiles: spec !== undefined,
    },
    open(current) {
      const client = createModuleClient(channel(current));
      const settings: DynamicSettings | undefined = schema ? client.settings(schema) : undefined;
      const { files } = client;
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
        Page: definition ? () => <Dynamic component={definition.Page} client={client} /> : undefined,
      };
    },
  };
}

/** The pages of the modules that have a file panel declared in module.toml: they show whether or not the module is loaded with settings. */
export function moduleFilePages(app: TransferApp) {
  return new Map(
    Object.keys(MODULE_FILES).map((id) => [
      id,
      moduleFilePage(app, id, {
        channel: (current) =>
          createDeviceModuleChannel({
            settings: deviceSettingsChannel(id),
            files: bindModuleFiles(id, app.target.bindSession(), current),
          }),
      }),
    ]),
  );
}
