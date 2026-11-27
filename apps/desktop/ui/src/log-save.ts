// The Save button of the Log drawer: the visible entries as a text file through the host's save dialog.
import { saveName, saveText, type LogEntry } from './log-core';
import type { FsHost } from '@digi/workbench-core/host/host.ts';

/** Save `entries`; the path the user chose, or null when the dialog was cancelled. */
export function saveLog(fs: FsHost, entries: LogEntry[], now: number): Promise<string | null> {
  return fs.saveLog(new TextEncoder().encode(saveText(entries, now)), saveName(now));
}
