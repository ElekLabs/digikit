// The desktop's save of module-page files: one native command opens the system folder dialog and writes the packed list into the folder the
// user picks; the webview never names a folder.
import { checkSaveFiles, type SaveFile, type SaveResult } from '@digi/module-ui';

/**
 * The list as one raw body for `files_save_with_dialog`: per file a 4-byte big-endian name length, the UTF-8 name, a 4-byte big-endian size and the
 * bytes. The Rust side refuses a name that is not a plain file name before it writes the first file.
 */
export function packFiles(files: SaveFile[]): Uint8Array {
  const names = files.map((f) => new TextEncoder().encode(f.name));
  const total = files.reduce((sum, f, i) => sum + 8 + names[i].length + f.bytes.length, 0);
  const out = new Uint8Array(total);
  const view = new DataView(out.buffer);
  let at = 0;
  files.forEach((f, i) => {
    view.setUint32(at, names[i].length);
    out.set(names[i], at + 4);
    at += 4 + names[i].length;
    view.setUint32(at, f.bytes.length);
    out.set(f.bytes, at + 4);
    at += 4 + f.bytes.length;
  });
  return out;
}

export interface SaveFilesDeps {
  /** `files_save_with_dialog`: opens the folder dialog, writes the packed list into the folder picked and returns the result (cancelled: nothing written). */
  saveWithDialog(packed: Uint8Array): Promise<SaveResult>;
}

/** Checks the list (names and limits, as the native side does again), then one native call; a file of the same name is overwritten. */
export function createSaveFiles(deps: SaveFilesDeps): (files: SaveFile[]) => Promise<SaveResult> {
  return async (files) => {
    checkSaveFiles(files);
    return deps.saveWithDialog(packFiles(files));
  };
}
