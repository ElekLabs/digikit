// Saving files to the user's computer from a module page: the data types, the one name check and the page-side helpers.

/** One file a page asks the app to save. */
export interface SaveFile {
  name: string;
  bytes: Uint8Array;
}

/** What a save did. `cancelled`: the user closed the folder choice and nothing was written. `where` names the folder (or the download place) when the app knows it. */
export interface SaveResult {
  saved: number;
  cancelled: boolean;
  where?: string;
}

/** The reason a name or list is refused. */
export class SaveFilesError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'SaveFilesError';
  }
}

/** The limits of one save call; the desktop's Rust side holds the same numbers (`apps/desktop/src-tauri/src/files_save.rs`). */
export const SAVE_LIMITS = { maxNameBytes: 255, maxFiles: 256, maxTotalBytes: 64 * 1024 * 1024 } as const;

const RESERVED = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/i;

/**
 * The one check of a save name, the same rule as the Rust side (tests/fixtures/save-names.json holds the accept and refuse cases of both): not empty,
 * at most 255 UTF-8 bytes, none of `/ \ : * ? " < > |`, no control character (below 0x20, 0x7f), not only dots (so not `.` or `..`), no trailing dot or
 * space, and not a Windows reserved device name (CON, PRN, AUX, NUL, COM1-9, LPT1-9), with or without an extension. `a..b` is a plain name.
 * Throws `SaveFilesError`.
 */
export function checkSaveName(name: string): void {
  if (name.length === 0) throw new SaveFilesError('A file name is empty.');
  if (new TextEncoder().encode(name).length > SAVE_LIMITS.maxNameBytes)
    throw new SaveFilesError(`The file name is longer than ${SAVE_LIMITS.maxNameBytes} bytes: ${name.slice(0, 40)}`);
  // eslint-disable-next-line no-control-regex
  if (/[/\\:*?"<>|\u0000-\u001f\u007f]/.test(name)) throw new SaveFilesError(`Not a plain file name: ${name}`);
  if (/^\.+$/.test(name) || /[. ]$/.test(name)) throw new SaveFilesError(`Not a plain file name: ${name}`);
  if (RESERVED.test(name.split('.')[0]!.trimEnd())) throw new SaveFilesError(`A reserved device name: ${name}`);
}

/**
 * Checks every name of a save, that no two names differ only in case, and the limits (at most 256 files, 64 MiB in all). Throws `SaveFilesError`
 * before anything is written.
 */
export function checkSaveFiles(files: readonly SaveFile[]): void {
  if (files.length > SAVE_LIMITS.maxFiles)
    throw new SaveFilesError(`Save at most ${SAVE_LIMITS.maxFiles} files at once.`);
  let total = 0;
  const seen = new Set<string>();
  for (const { name, bytes } of files) {
    checkSaveName(name);
    const key = name.toLowerCase();
    if (seen.has(key)) throw new SaveFilesError(`The file name is used twice: ${name}`);
    seen.add(key);
    total += bytes.length;
    if (total > SAVE_LIMITS.maxTotalBytes)
      throw new SaveFilesError(`Save at most ${SAVE_LIMITS.maxTotalBytes} bytes at once.`);
  }
}

/** The page-side helpers: they check the names, then send the files to the channel. */
export interface ModuleSaveFiles {
  /** Saves the files into one folder the user picks. An existing file of the same name is overwritten after the user has picked the folder; the app refuses a name that is not a plain file name and a list over the limits. */
  saveFiles(files: SaveFile[]): Promise<SaveResult>;
  saveFile(name: string, bytes: Uint8Array): Promise<SaveResult>;
}
