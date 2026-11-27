import { DefaultModuleError } from '@digi/workbench-core/firmware/default-modules-core.ts';
import { createFirmwareApp } from '@digi/workbench-core/firmware/app.ts';
import type { DesktopApps } from '../src/desktop-app';

export function previewFirmware(apps: DesktopApps) {
  // The default modules install on a fake device: ?defmods=ok (default) | slow (each step takes 3 s) | fail (the module cannot be uploaded)
  const defmods = new URLSearchParams(location.search).get('defmods') ?? 'ok';
  return createFirmwareApp(
    {
      ...apps.firmware.env,
      defaultModules: {
        entries: apps.firmware.env.defaultModules!.entries,
        async install(where, _device, ids, progress, _opts) {
          const wait = (ms: number) => new Promise((r) => setTimeout(r, defmods === 'slow' ? ms * 4 : ms));
          progress({ text: 'Waiting for the runtime to answer', done: 0, total: ids.length + 2 });
          await wait(700);
          for (const [i, id] of ids.entries()) {
            progress({ text: `Installing ${id} (${i + 1} of ${ids.length})`, done: i + 1, total: ids.length + 2 });
            await wait(900);
            if (defmods === 'fail') throw new DefaultModuleError(id, 'E_NO_SPACE: the card has no room for it');
          }
          progress({
            text: `Enabling ${ids.join(', ')}: restarting the runtime`,
            done: ids.length + 1,
            total: ids.length + 2,
          });
          await wait(1200);
          return [...ids.map((id) => `${id}: uploaded`), `enabled ${ids.join(', ')} (restarted once, ${where})`];
        },
      },
    },
    apps.app,
  );
}
