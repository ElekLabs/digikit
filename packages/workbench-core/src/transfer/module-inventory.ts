import { createSignal } from 'solid-js';
import { readLoadedModules } from '../modules/dktransfer';

// A reader belongs to the connection on which the caller started its operation.
export function createModuleInventory() {
  const [modules, setModules] = createSignal<Awaited<ReturnType<typeof readLoadedModules>>>([]);
  // Whether the list is an answer: false from a clear until a read of this connection ends, and while a read runs.
  const [known, setKnown] = createSignal(false);
  const [reading, setReading] = createSignal(0);
  let generation = 0;
  let sequence = 0;
  function clear() {
    generation++;
    setKnown(false);
    setModules([]);
  }
  function reader(): typeof readLoadedModules {
    const connection = generation;
    return async (session, info, pkg) => {
      const request = connection === generation ? ++sequence : -1;
      setReading((n) => n + 1);
      try {
        const loaded = await readLoadedModules(session, info, pkg);
        if (connection === generation && request === sequence) {
          setModules(loaded);
          setKnown(true);
        }
        return loaded;
      } catch (error) {
        // A failed read leaves nothing listed: that is an answer, not a refresh in progress.
        if (connection === generation && request === sequence) setKnown(true);
        throw error;
      } finally {
        setReading((n) => n - 1);
      }
    };
  }
  /** True when the list is the result of a finished read and no read is running: a module missing from it is gone. */
  const settled = () => known() && reading() === 0;
  return { modules, settled, clear, reader };
}
