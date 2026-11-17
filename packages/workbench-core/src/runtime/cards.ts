// Sample cards for the Digitakt II (desktop app only for now): the files of a chosen +Drive-style folder go to the
// native controller, which builds a +Drive card in process, boots the DT2 with it and reports progress.

/** What the controller's `card.status` reports. */
export interface CardStatus {
  state: 'idle' | 'working' | 'ready' | 'error';
  phase?: 'scanning' | 'building' | 'booting' | 'restoring' | 'ready';
  message?: string;
  /** 0..1 */
  fraction?: number;
  samples?: number;
  /** The chosen folder's name. */
  name?: string;
  /** Mounted again from the last session rather than chosen now. */
  auto?: boolean;
  card_cached?: boolean;
  ready_cached?: boolean;
  seconds?: number;
}

/**
 * Every file of the chosen folder with its path inside it (`kicks/a.wav`, `.mmcfs/projects/0000.bin`): the folder is
 * a `plus_drive/dt2/`-style folder and the controller builds the card from all of it. `.DS_Store` is left out.
 */
export function folderFiles(files: Iterable<File>): { path: string; file: File }[] {
  const out: { path: string; file: File }[] = [];
  for (const file of files) {
    const parts = (file.webkitRelativePath || file.name).split('/');
    const inside = parts.length > 1 ? parts.slice(1) : parts;
    if (file.name === '.DS_Store' || inside.some((p) => p === '' || p === '.' || p === '..')) continue;
    out.push({ path: inside.join('/'), file });
  }
  return out.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
}

/** Shown while the DT2 runs without a card. */
export const SAMPLES_HINT = 'Choose samples to load a folder of WAVs';

/** The name of the folder the files were chosen from, or '' for loose files. */
export function folderName(files: Iterable<{ webkitRelativePath?: string }>): string {
  for (const file of files) {
    const parts = (file.webkitRelativePath ?? '').split('/');
    if (parts.length > 1 && parts[0]) return parts[0];
  }
  return '';
}

/** One line for the toolbar. */
export function cardLine(status: CardStatus | undefined): string {
  if (!status) return '';
  if (status.state === 'idle') return SAMPLES_HINT;
  if (status.state === 'error') return `Card failed: ${status.message ?? 'unknown error'}`;
  const percent = Math.round(100 * (status.fraction ?? 0));
  if (status.state === 'ready') {
    return `Samples: ${status.name || 'card'} (${status.samples ?? '?'} file${status.samples === 1 ? '' : 's'})`;
  }
  return `${status.message ?? 'Working'}${status.message?.includes('%') ? '' : ` (${percent}%)`}`;
}

/** The native bridge: stage the files, start the job, poll its status. */
export interface CardBridge {
  reset(): Promise<void>;
  stage(name: string, bytes: Uint8Array): Promise<void>;
  load(name?: string): Promise<unknown>;
  status(): Promise<CardStatus>;
}

/** Send `files` to the controller and start the card job; returns when the job has started. */
export async function startCard(
  bridge: CardBridge,
  files: (File | { path: string; file: File })[],
  onStaged?: (done: number, total: number) => void,
  name = folderName(files.map((f) => ('file' in f ? f.file : f))),
): Promise<void> {
  await bridge.reset();
  let done = 0;
  for (const entry of files) {
    const [path, file] = 'file' in entry ? [entry.path, entry.file] : [entry.name, entry];
    await bridge.stage(path, new Uint8Array(await file.arrayBuffer()));
    onStaged?.(++done, files.length);
  }
  await bridge.load(name);
}
