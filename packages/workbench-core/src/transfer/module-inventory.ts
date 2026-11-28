import { createSignal } from 'solid-js';
import { readLoadedModules } from '../modules/dktransfer';

// A reader belongs to the connection on which the caller started its operation.
export function createModuleInventory() {
  const [modules, setModules] = createSignal<Awaited<ReturnType<typeof readLoadedModules>>>([]);
  // Whether the list is an answer: false from a clear until a read of this connection ends, and while a read runs.
  const [known, setKnown] = createSignal(false);
  const [reading, setReading] = createSignal(0);
  // True from a lost request until a read of the connection ends: the list is kept (a transient timeout must not flicker the pages) but not trusted.
  const [stale, setStale] = createSignal(false);
  let generation = 0;
  let sequence = 0;
  function clear() {
    generation++;
    setKnown(false);
    setStale(false);
    setModules([]);
  }
  /** The target cannot be reached: nothing is loaded that anyone can use, and that is an answer (a module page leaves). */
  function empty() {
    generation++;
    setKnown(true);
    setStale(false);
    setModules([]);
  }
  /** A request was lost: keep the list, mark it stale. The caller re-reads (`refresh`). */
  function markStale() {
    setStale(true);
  }
  /**
   * Read the inventory of the connection: `open` gives the session and its INFO. Any failure, INFO and a stale connection included,
   * empties the list; success replaces it. A reply of an older connection or read is dropped.
   */
  async function refresh(
    open: () => Promise<{
      session: Parameters<typeof readLoadedModules>[0];
      info: Parameters<typeof readLoadedModules>[1];
    }>,
  ): Promise<void> {
    const connection = generation;
    try {
      const { session, info } = await open();
      await reader()(session, info);
    } catch {
      if (connection === generation) empty();
    }
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
          setStale(false);
        }
        return loaded;
      } catch (error) {
        // A failed read leaves nothing listed: that is an answer, not a refresh in progress.
        if (connection === generation && request === sequence) {
          setModules([]);
          setKnown(true);
          setStale(false);
        }
        throw error;
      } finally {
        setReading((n) => n - 1);
      }
    };
  }
  /** True when the list is the result of a finished read and no read is running: a module missing from it is gone. */
  const settled = () => known() && reading() === 0;
  return { modules, settled, stale, clear, empty, markStale, refresh, reader };
}
