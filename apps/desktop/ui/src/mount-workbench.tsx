import { createAnalyzerWorkers } from './workers';
import { createTheme } from './theme';
import { render } from 'solid-js/web';
import '@fontsource-variable/archivo/wdth.css';
import '@fontsource/jetbrains-mono/400.css';
import '@fontsource/jetbrains-mono/500.css';
import '@digi/workbench-core/ui/styles/site.css';
import './console.css';
import './native.css';
import './tooltip.css';
import Console, { type ConsoleOptions } from './Console';
import type { DesktopApps } from './desktop-app';
import type { Host } from '@digi/workbench-core/host/host.ts';
import { installNativeFeel } from './native';
import { installTooltips } from './tooltip';
import AppProvider from '@digi/workbench-core/ui/transfer/AppProvider.tsx';

export function mountWorkbench(host: Host, apps: DesktopApps, options: ConsoleOptions = {}) {
  const analyzerWorkers = createAnalyzerWorkers();
  const theme = createTheme(apps.app.storage, document.documentElement);
  installNativeFeel();
  installTooltips();

  render(
    () => (
      <AppProvider app={apps.app} firmware={apps.firmware}>
        <Console {...options} theme={theme} host={host} analyzerWorkers={analyzerWorkers} tools={apps} />
      </AppProvider>
    ),
    document.getElementById('app')!,
  );
}
