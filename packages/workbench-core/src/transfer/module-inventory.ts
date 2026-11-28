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
  /** The target cannot be reached: nothing is loaded that anyone can use, and that is an answer (a module page leaves). */
  function empty() {
    generation++;
    setKnown(true);
    setModules([]);
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
    const request = ++sequence;
    try {
      await read(connection, request, async () => {
        const { session, info } = await open();
        return readLoadedModules(session, info);
      });
    } catch {
      /* the failure is already the answer */
    }
  }
  // A read belongs to the connection and the request number it started with: only the newest read of the current connection may answer.
  async function read(
    connection: number,
    request: number,
    work: () => ReturnType<typeof readLoadedModules>,
  ): ReturnType<typeof readLoadedModules> {
    setReading((n) => n + 1);
    try {
      const loaded = await work();
      if (connection === generation && request === sequence) {
        setModules(loaded);
        setKnown(true);
      }
      return loaded;
    } catch (error) {
      // A failed read leaves nothing listed: that is an answer, not a refresh in progress.
      if (connection === generation && request === sequence) {
        setModules([]);
        setKnown(true);
      }
      throw error;
    } finally {
      setReading((n) => n - 1);
    }
  }
  function reader(): typeof readLoadedModules {
    const connection = generation;
    return (session, info, pkg) => {
      const request = connection === generation ? ++sequence : -1;
      return read(connection, request, () => readLoadedModules(session, info, pkg));
    };
  }
  /** True when the list is the result of a finished read and no read is running: a module missing from it is gone. */
  const settled = () => known() && reading() === 0;
  return { modules, settled, clear, empty, refresh, reader };
}
