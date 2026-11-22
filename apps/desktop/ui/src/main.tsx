import { createDesktopApps } from './desktop-app';
import { createTauriHost } from './host/tauri-host';
import { mountWorkbench } from './mount-workbench';

const host = createTauriHost();
mountWorkbench(host, createDesktopApps(host));
