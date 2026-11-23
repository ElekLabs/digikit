// One window's firmware action, exposed to other pages through the Transfer environment.
export function createEmulatorActions() {
  let reloadWith: (() => void) | undefined;

  return {
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
