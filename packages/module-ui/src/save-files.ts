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

/** The one check of a save name: a plain file name, no path separator, no `..` and no control character. Throws `SaveFilesError`. */
export function checkSaveName(name: string): void {
  if (name.length === 0) throw new SaveFilesError('A file name is empty.');
  if (name === '.' || name === '..' || /[/\\\0]/.test(name) || name.includes('..'))
    throw new SaveFilesError(`Not a plain file name: ${name}`);
}

/** Checks every name of a save and that no name repeats. Throws `SaveFilesError`. */
export function checkSaveFiles(files: readonly SaveFile[]): void {
  const seen = new Set<string>();
  for (const { name } of files) {
    checkSaveName(name);
    if (seen.has(name)) throw new SaveFilesError(`The file name is used twice: ${name}`);
    seen.add(name);
  }
}

/** The page-side helpers: they check the names, then send the files to the channel. */
export interface ModuleSaveFiles {
  /** Saves the files into one folder the user picks. An existing file of the same name is overwritten after the user has picked the folder. */
  saveFiles(files: SaveFile[]): Promise<SaveResult>;
  saveFile(name: string, bytes: Uint8Array): Promise<SaveResult>;
}
