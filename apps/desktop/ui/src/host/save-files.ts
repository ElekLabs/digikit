// The desktop's save of module-page files: the system folder dialog once, then one native write of the packed list into that folder.
import { checkSaveFiles, type SaveFile, type SaveResult } from '@digi/module-ui';

/**
 * The list as one raw body for `files_save_to_folder`: per file a 4-byte big-endian name length, the UTF-8 name, a 4-byte big-endian size and the
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
  /** The system's folder dialog; null when the user cancels. */
  pickFolder(): Promise<string | null>;
  /** Writes the packed list into the folder; the number of files written. */
  write(folder: string, packed: Uint8Array): Promise<number>;
}

/** One folder dialog, then every file written into the folder; a file of the same name is overwritten. Cancelling writes nothing. */
export function createSaveFiles(deps: SaveFilesDeps): (files: SaveFile[]) => Promise<SaveResult> {
  return async (files) => {
    checkSaveFiles(files);
    const folder = await deps.pickFolder();
    if (folder === null) return { saved: 0, cancelled: true };
    return { saved: await deps.write(folder, packFiles(files)), cancelled: false, where: folder };
  };
}
