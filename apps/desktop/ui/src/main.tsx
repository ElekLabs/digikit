import { render } from 'solid-js/web';
import '@fontsource-variable/archivo/wdth.css';
import '@fontsource/jetbrains-mono/400.css';
import '@fontsource/jetbrains-mono/500.css';
import '@web/styles/site.css';
import './console.css';
import './native.css';
import './tooltip.css';
import Console from './Console';
import { installDesktopEnv } from './desktop-env';
import { createTauriHost } from './host/tauri-host';
import { installNativeFeel } from './native';
import { installTooltips } from './tooltip';

const host = createTauriHost();
installDesktopEnv(host);
installNativeFeel();
installTooltips();

render(() => <Console host={host} />, document.getElementById('app')!);
