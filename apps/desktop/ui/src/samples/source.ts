import { createSignal } from 'solid-js';
import { sourceKind, type Prepared } from '@digi/dk-session/dksample';
import { errorLine } from '@digi/workbench-core/modules/backend.ts';
import { remembered, type KeyValue } from '@digi/workbench-core/storage.ts';
import { listingChanged } from '@digi/workbench-core/transfer/refresh-core.ts';
import { joinPath } from '@digi/workbench-core/transfer/sample-upload.ts';
import { localRows, selectedRows, type LocalEntry } from '@digi/workbench-core/transfer/samples-core.ts';
import type { SampleSourceFiles } from './source-files';

export const SAMPLE_FOLDER_KEY = 'digikit.samplesFolder';

/** Read only enough of the first 400 files at a level to label what the upload will do. */
const LABELLED = 400;

async function labelOf(read: () => Promise<Uint8Array>, size: number): Promise<Prepared['kind'] | undefined> {
  try {
    return sourceKind(await read(), size);
  } catch {
    return undefined;
  }
}

export function createSamplesSource(files: SampleSourceFiles, storage: KeyValue) {
  const [folder, rememberFolder] = remembered(storage, SAMPLE_FOLDER_KEY, (v) => v ?? '');
  const [path, setPath] = createSignal('');
  const [entries, setEntries] = createSignal<LocalEntry[]>();
  const [kinds, setKinds] = createSignal<Record<string, Prepared['kind']>>({});
  const [added, setAdded] = createSignal<File[]>([]);
  const [selected, setSelected] = createSignal<Set<string>>(new Set());
  const rows = () => localRows(path(), entries() ?? [], added(), kinds());
  const picked = () => selectedRows(rows(), selected());
  let revision = 0;
  let readGeneration = 0;

  async function read() {
    const generation = ++readGeneration;
    const root = folder();
    const rel = path();
    const current = () => generation === readGeneration;
    if (!root) {
      setEntries(undefined);
      return;
    }
    let listing: LocalEntry[] | undefined;
    try {
      listing = await files.list(root, rel, 'read');
    } catch (e) {
      if (!current()) return;
      setEntries([]);
      throw new Error(errorLine(e), { cause: e });
    }
    if (!current()) return;
    setEntries(listing);
    const labelled = (listing ?? []).filter((e) => e.kind === 'file').slice(0, LABELLED);
    const next: Record<string, Prepared['kind']> = {};
    for (let i = 0; i < labelled.length; i += 16) {
      if (!current()) return;
      await Promise.all(
        labelled.slice(i, i + 16).map(async (f) => {
          const key = joinPath(rel, f.name);
          const kind = await labelOf(() => files.read(root, key, 64), f.size);
          if (kind) next[key] = kind;
        }),
      );
    }
    if (!current()) return;
    setKinds((all) => ({
      ...Object.fromEntries(Object.entries(all).filter(([key]) => key.startsWith('+'))),
      ...next,
    }));
  }

  async function changed() {
    const root = folder();
    const rel = path();
    const generation = readGeneration;
    if (!root) return false;
    try {
      const listing = await files.list(root, rel, 'poll');
      return generation === readGeneration && !!listing && listingChanged(entries(), listing);
    } catch {
      return false;
    }
  }

  function select(next: Set<string>) {
    revision++;
    setSelected(next);
  }

  async function open(next: string) {
    setPath(next);
    select(new Set());
    await read();
  }

  async function choose() {
    const next = await files.pick(folder());
    if (next) {
      rememberFolder(next);
      await open('');
    }
  }

  /** Files added by hand join the list (same name: the newer one) and are selected. */
  function add(incoming: File[]) {
    if (!incoming.length) return;
    const names = new Set(incoming.map((f) => f.name));
    setAdded((old) => [...old.filter((f) => !names.has(f.name)), ...incoming]);
    select(new Set([...selected(), ...incoming.map((f) => `+${f.name}`)]));
    setKinds((all) =>
      Object.fromEntries(Object.entries(all).filter(([key]) => !key.startsWith('+') || !names.has(key.slice(1)))),
    );
    for (const file of incoming) {
      void labelOf(async () => new Uint8Array(await file.slice(0, 64).arrayBuffer()), file.size).then((kind) => {
        if (kind && added().findLast((f) => f.name === file.name) === file)
          setKinds((all) => ({ ...all, [`+${file.name}`]: kind }));
      });
    }
  }

  /** Pin the source of planned reads even if a new folder or same-named added file replaces it. */
  function snapshot() {
    const root = folder();
    const initialFilesByName = new Map(added().map((f) => [f.name, f]));
    const captured = revision;
    return {
      list: async (rel: string) => (await files.list(root, rel, 'walk')) ?? [],
      async bytes(rel: string, name: string, fromAdded: boolean) {
        if (!fromAdded) return files.read(root, rel);
        const file = initialFilesByName.get(name);
        if (!file) throw new Error(`The added file ${name} is no longer available.`);
        return new Uint8Array(await file.arrayBuffer());
      },
      clearSelection() {
        if (captured === revision) select(new Set());
      },
    };
  }

  return { folder, path, entries, rows, selected, picked, select, open, choose, add, read, changed, snapshot };
}
