import { For, Show, createSignal, untrack } from 'solid-js';
import type { KeyValue } from '@digi/workbench-core/storage.ts';
import { FIRMWARE, FOLDER, Icon, MODULES, PLAY, REMOTE, SETTINGS } from '../icons';
import type { Page } from '../page-core';
import { createPrefs } from '../prefs';

const SIDEBAR_KEY = 'digikit.sidebar';

export default function Navigation(props: {
  storage: KeyValue;
  page: Page;
  isEmulator: boolean;
  modules: readonly { id: string; name: string }[];
  moduleId: string;
  onNavigate(page: Exclude<Page, 'module'>): void;
  onOpenModule(id: string): void;
}) {
  const { readPref, writePref } = createPrefs(untrack(() => props.storage));
  const [sidebarOpen, setSidebarOpen] = createSignal(readPref(SIDEBAR_KEY) !== 'closed');

  return (
    <nav class="sidebar" classList={{ closed: !sidebarOpen() }} aria-label="Pages">
      <button
        type="button"
        class="side toggle"
        aria-expanded={sidebarOpen()}
        aria-label={sidebarOpen() ? 'Collapse the sidebar' : 'Expand the sidebar'}
        title={sidebarOpen() ? 'Collapse the sidebar' : 'Expand the sidebar'}
        onClick={() => {
          setSidebarOpen(!sidebarOpen());
          writePref(SIDEBAR_KEY, sidebarOpen() ? 'open' : 'closed');
        }}
      >
        <Icon d={sidebarOpen() ? 'M10 3L5 8L10 13' : 'M6 3L11 8L6 13'} />
      </button>
      <span class="side-group">Run</span>
      <button
        type="button"
        class="side"
        classList={{ on: props.page === 'run' }}
        aria-current={props.page === 'run' ? 'page' : undefined}
        title={props.isEmulator ? 'The emulator' : 'Control the selected device'}
        onClick={() => props.onNavigate('run')}
      >
        <Icon d={props.isEmulator ? PLAY : REMOTE} />
        <span>{props.isEmulator ? 'Emulator' : 'Remote'}</span>
      </button>
      <span class="side-group">Device</span>
      <button
        type="button"
        class="side"
        classList={{ on: props.page === 'transfer' }}
        aria-current={props.page === 'transfer' ? 'page' : undefined}
        title="Browse local modules and transfer files to the selected target"
        onClick={() => props.onNavigate('transfer')}
      >
        <Icon d={MODULES} />
        <span>Transfer</span>
      </button>
      <button
        type="button"
        class="side"
        classList={{ on: props.page === 'files' }}
        aria-current={props.page === 'files' ? 'page' : undefined}
        title="See every file in the target's card store: space, sizes and module headers"
        onClick={() => props.onNavigate('files')}
      >
        <Icon d={FOLDER} />
        <span>Files</span>
      </button>
      <button
        type="button"
        class="side"
        classList={{ on: props.page === 'firmware' }}
        aria-current={props.page === 'firmware' ? 'page' : undefined}
        title="Patch the stock firmware and flash it to a device over MIDI, or go back to stock"
        onClick={() => props.onNavigate('firmware')}
      >
        <Icon d={FIRMWARE} />
        <span>Firmware</span>
      </button>
      <Show when={props.modules.length}>
        <span class="side-group">Modules</span>
        <For each={props.modules}>
          {(item) => (
            <button
              type="button"
              class="side"
              classList={{ on: props.page === 'module' && props.moduleId === item.id }}
              aria-current={props.page === 'module' && props.moduleId === item.id ? 'page' : undefined}
              title={item.name}
              onClick={() => props.onOpenModule(item.id)}
            >
              <Icon d={MODULES} />
              <span>{item.name}</span>
            </button>
          )}
        </For>
      </Show>
      <button
        type="button"
        class="side settings-link"
        classList={{ on: props.page === 'settings' }}
        aria-current={props.page === 'settings' ? 'page' : undefined}
        aria-label="Settings"
        title="Settings"
        onClick={() => props.onNavigate('settings')}
      >
        <Icon d={SETTINGS} />
        <span>Settings</span>
      </button>
    </nav>
  );
}
