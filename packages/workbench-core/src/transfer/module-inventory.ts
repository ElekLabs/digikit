import { createSignal } from 'solid-js';
import { readLoadedModules } from '../modules/dktransfer';

// A reader belongs to the connection on which the caller started its operation.
export function createModuleInventory() {
  const [modules, setModules] = createSignal<Awaited<ReturnType<typeof readLoadedModules>>>([]);
  let generation = 0;
  let sequence = 0;
  function clear() {
    generation++;
    setModules([]);
  }
  function reader(): typeof readLoadedModules {
    const connection = generation;
    return async (session, info) => {
      const request = connection === generation ? ++sequence : -1;
      const loaded = await readLoadedModules(session, info);
      if (connection === generation && request === sequence) setModules(loaded);
      return loaded;
    };
  }
  return { modules, clear, reader };
}
