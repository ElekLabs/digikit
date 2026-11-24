import type { FirmwareInfo } from '@digi/workbench-core/runtime/runtime.ts';

// One window's firmware action, exposed to other pages through the Transfer environment.
export function createEmulatorActions() {
  let reloadWith: (() => void) | undefined;
  let flashUi:
    { paused(): boolean; emulatorGeneration(): number; refreshAfterFlash(info: FirmwareInfo): void } | undefined;

  return {
    paused: () => flashUi?.paused() ?? false,
    emulatorGeneration: () => flashUi?.emulatorGeneration() ?? 0,
    refreshAfterFlash: (info: FirmwareInfo) => flashUi?.refreshAfterFlash(info),
    registerFlashUi(ui: NonNullable<typeof flashUi>) {
      flashUi = ui;
      return () => {
        if (flashUi === ui) flashUi = undefined;
      };
    },
    reloadWithRuntime: () => reloadWith?.(),
    registerReloadWithRuntime(command: () => void) {
      const registered = () => command();
      reloadWith = registered;
      return () => {
        if (reloadWith === registered) reloadWith = undefined;
      };
    },
  };
}
