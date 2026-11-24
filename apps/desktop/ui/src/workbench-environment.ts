import type { Host } from '@digi/workbench-core/host/host.ts';
import type { WorkbenchShellOptions } from './WorkbenchShell';
import { createDesktopApps, type DesktopApps } from './desktop-app';
import { createTauriHost } from './host/tauri-host';

export interface WorkbenchEnvironment {
  host: Host;
  apps: DesktopApps;
  options?: WorkbenchShellOptions;
}

export function createWorkbenchEnvironment(): WorkbenchEnvironment {
  const host = createTauriHost();
  return { host, apps: createDesktopApps(host) };
}
