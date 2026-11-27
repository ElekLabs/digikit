// The site's save of module-page files (`FsHost.saveFiles` shape): with a directory picker (Chrome, Edge) one folder choice and each file written into it;
// without one, one download per file. The app object builds it with the browser's functions; tests give fakes.
import { checkSaveFiles, type SaveFile, type SaveResult } from '@digi/module-ui';

/** The part of a File System Access directory handle the save uses. */
export interface SaveDirectory {
  name: string;
  getFileHandle(
    name: string,
    options: { create: true },
  ): Promise<{
    createWritable(): Promise<{
      write(data: Uint8Array): Promise<void>;
      close(): Promise<void>;
      abort(): Promise<void>;
    }>;
  }>;
}

export interface BrowserSaveDeps {
  /** `window.showDirectoryPicker` bound to the window, or undefined where the browser has none. It rejects with an `AbortError` when the user cancels. */
  pickDirectory?: (options: { mode: 'readwrite' }) => Promise<SaveDirectory>;
  /** Starts the download of one file (an anchor click on an object URL). */
  download(name: string, bytes: Uint8Array): void;
}

/** A write into the folder failed after `saved` files were complete; `cause` is the failure. */
export class PartialSaveError extends Error {
  constructor(
    readonly saved: number,
    readonly total: number,
    override readonly cause: unknown,
  ) {
    super(`Saved ${saved} of ${total} files, then failed: ${cause instanceof Error ? cause.message : String(cause)}`);
    this.name = 'PartialSaveError';
  }
}

/** How long a download's object URL stays valid: long enough for the browser to start reading it. */
export const DOWNLOAD_URL_LIFETIME_MS = 60_000;

/**
 * Writes into the chosen folder, replacing a file of the same name (the user chose the folder just before). Without a picker the browser downloads
 * each file, so `where` is the browser's download place and is not named. A failed write aborts that file's writable (no half file is kept) and
 * throws `PartialSaveError` with the number of files written before it.
 */
export function createBrowserSaveFiles(deps: BrowserSaveDeps): (files: SaveFile[]) => Promise<SaveResult> {
  return async (files) => {
    checkSaveFiles(files);
    if (!deps.pickDirectory) {
      for (const { name, bytes } of files) deps.download(name, bytes);
      return { saved: files.length, cancelled: false };
    }
    let dir: SaveDirectory;
    try {
      dir = await deps.pickDirectory({ mode: 'readwrite' });
    } catch (e) {
      if (e instanceof Error && e.name === 'AbortError') return { saved: 0, cancelled: true };
      throw e;
    }
    let saved = 0;
    for (const { name, bytes } of files) {
      let writable:
        Awaited<ReturnType<Awaited<ReturnType<SaveDirectory['getFileHandle']>>['createWritable']>> | undefined;
      try {
        writable = await (await dir.getFileHandle(name, { create: true })).createWritable();
        await writable.write(bytes);
        await writable.close();
      } catch (e) {
        await writable?.abort().catch(() => {});
        throw new PartialSaveError(saved, files.length, e);
      }
      saved += 1;
    }
    return { saved: files.length, cancelled: false, where: dir.name };
  };
}

/** The browser's own functions for `createBrowserSaveFiles`. */
export function browserSaveDeps(win: Window & typeof globalThis): BrowserSaveDeps {
  const picker = (win as unknown as { showDirectoryPicker?: BrowserSaveDeps['pickDirectory'] }).showDirectoryPicker;
  return {
    pickDirectory: picker?.bind(win),
    download(name, bytes) {
      const url = win.URL.createObjectURL(new Blob([bytes as BlobPart]));
      const a = win.document.createElement('a');
      a.href = url;
      a.download = name;
      a.click();
      win.setTimeout(() => win.URL.revokeObjectURL(url), DOWNLOAD_URL_LIFETIME_MS);
    },
  };
}
