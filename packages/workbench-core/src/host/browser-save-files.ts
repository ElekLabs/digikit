// The site's save of module-page files (`FsHost.saveFiles` shape): with a directory picker (Chrome, Edge) one folder choice and each file written into it;
// without one, one download per file. The app object builds it with the browser's functions; tests give fakes.
import { checkSaveFiles, type SaveFile, type SaveResult } from '@digi/module-ui';

/** The part of a File System Access directory handle the save uses. */
export interface SaveDirectory {
  name: string;
  getFileHandle(
    name: string,
    options: { create: true },
  ): Promise<{ createWritable(): Promise<{ write(data: Uint8Array): Promise<void>; close(): Promise<void> }> }>;
}

export interface BrowserSaveDeps {
  /** `window.showDirectoryPicker` bound to the window, or undefined where the browser has none. It rejects with an `AbortError` when the user cancels. */
  pickDirectory?: (options: { mode: 'readwrite' }) => Promise<SaveDirectory>;
  /** Starts the download of one file (an anchor click on an object URL). */
  download(name: string, bytes: Uint8Array): void;
}

/**
 * Writes into the chosen folder, replacing a file of the same name (the user chose the folder just before). Without a picker the browser downloads
 * each file, so `where` is the browser's download place and is not named.
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
    for (const { name, bytes } of files) {
      const writable = await (await dir.getFileHandle(name, { create: true })).createWritable();
      await writable.write(bytes);
      await writable.close();
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
      win.setTimeout(() => win.URL.revokeObjectURL(url), 0);
    },
  };
}
