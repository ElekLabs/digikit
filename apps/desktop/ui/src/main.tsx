import { render } from 'solid-js/web';
import '@fontsource-variable/archivo/wdth.css';
import '@fontsource/jetbrains-mono/400.css';
import '@fontsource/jetbrains-mono/500.css';
import '@digi/workbench-core/ui/styles/site.css';
import './console.css';
import './native.css';
import './tooltip.css';
import Console from './Console';
import { createDesktopApps } from './desktop-app';
import { createTauriHost } from './host/tauri-host';
import { installNativeFeel } from './native';
import { installTooltips } from './tooltip';
import AppProvider from '@digi/workbench-core/ui/transfer/AppProvider.tsx';

const host = createTauriHost();
const apps = createDesktopApps(host);
installNativeFeel();
installTooltips();

render(
  () => (
    <AppProvider app={apps.app} firmware={apps.firmware}>
      <Console host={host} />
    </AppProvider>
  ),
  document.getElementById('app')!,
);
