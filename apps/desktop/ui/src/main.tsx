import { render } from 'solid-js/web';
import '@fontsource-variable/archivo/wdth.css';
import '@fontsource/jetbrains-mono/400.css';
import '@fontsource/jetbrains-mono/500.css';
import '@web/styles/site.css';
import './console.css';
import './native.css';
import './tooltip.css';
import Console from './Console';
import { createDesktopApps } from './desktop-app';
import { createTauriHost } from './host/tauri-host';
import { installNativeFeel } from './native';
import { installTooltips } from './tooltip';
import AppProvider from '@web/transfer/AppProvider';

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
