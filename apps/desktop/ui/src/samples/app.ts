import type { SamplesKind } from '@digi/dk-session/samples';
import { errorLine, isCancelled } from '@digi/workbench-core/modules/backend.ts';
import { restartDevice } from '@digi/workbench-core/modules/dktransfer.ts';
import type { Host } from '@digi/workbench-core/host/host.ts';
import type { KeyValue } from '@digi/workbench-core/storage.ts';
import { noteActivity } from '@digi/workbench-core/transfer/activity.ts';
import type { SamplesSlot, TransferBulk } from '@digi/workbench-core/transfer/env.ts';
import type { CancelledRun } from '@digi/workbench-core/transfer/run-transfer.ts';
import {
  joinPath,
  uploadCancelled,
  uploadFailure,
  uploadItems,
  type UploadItem,
} from '@digi/workbench-core/transfer/sample-upload.ts';
import {
  deletePartLine,
  deleteRows,
  deletingLine,
  folderNameError,
  noFoldersMessage,
  planUpload,
  samplesBar,
  type LocalRow,
} from '@digi/workbench-core/transfer/samples-core.ts';
import { targetLabel, type TargetApp } from '@digi/workbench-core/transfer/target.ts';
import { writing } from '@digi/workbench-core/transfer/transfer-core.ts';
import { failureLine } from '@digi/workbench-core/transfer/wait-core.ts';
import type { LogWriter } from '../log-state';
import { readSamplePanes } from './read';
import { createSamplesDestination, type SamplePageState } from './destination';
import { createSampleDeviceSessions, type SampleConnection } from './device-session';
import { createSamplesSource } from './source';
import { createSampleSourceFiles, type SampleFileHost } from './source-files';

interface Dependencies {
  fs: SampleFileHost;
  firmware: Pick<Host['firmware'], 'sampleWasm'>;
  emulatorGeneration?: () => number;
  storage: KeyValue;
  getTarget: () => Pick<TargetApp, 'target' | 'switching' | 'bindSession'>;
  getState: () => SamplePageState;
  getConnection: () => SampleConnection;
  getBulk: () => TransferBulk | undefined;
  run: (what: string, job: () => Promise<void | CancelledRun>) => Promise<void>;
  writer: Pick<LogWriter, 'logEvent'>;
}

export function createSamplesApp({
  fs,
  firmware,
  emulatorGeneration,
  storage,
  getTarget,
  getState,
  getConnection,
  getBulk,
  run,
  writer,
}: Dependencies) {
  const source = createSamplesSource(createSampleSourceFiles(fs), storage);
  const destination = createSamplesDestination(
    getTarget,
    getState,
    createSampleDeviceSessions({
      sampleWasm: () => firmware.sampleWasm(),
      connection: getConnection,
      bulk: getBulk,
      writer,
    }),
    emulatorGeneration,
  );
  type Scope = NonNullable<ReturnType<typeof destination.capture>>;
  type Write = NonNullable<ReturnType<Scope['write']>>;
  let activeRead: { scope: Scope | undefined } | undefined;

  async function read() {
    const scope = destination.capture();
    const reading = { scope };
    activeRead = reading;
    try {
      await readSamplePanes(
        source.read,
        () => scope?.read() ?? Promise.resolve(),
        (e) => failureLine(e, targetLabel(scope?.binding.target ?? getTarget().target())),
      );
    } catch (e) {
      if (getState().activeMode() === 'samples') throw e;
    } finally {
      if (activeRead === reading) activeRead = undefined;
    }
  }

  const stop = (scope: Scope) => !scope.current() || getState().stopWasAsked();

  const send = async (scope: Scope, write: Write, mkdirs: string[], items: UploadItem[]) => {
    let done = false;
    await run('Uploading', async () => {
      if (!write.current()) return;
      const backend = await write.open('upload');
      scope.check();
      if (mkdirs.length) {
        const canMkdir = await backend.canMkdir();
        scope.check();
        if (!canMkdir) throw new Error(noFoldersMessage(write.name));
      }
      scope.beginUpload();
      const started = Date.now();
      await uploadItems(
        backend,
        (name, bytes) => backend.prepare(name, bytes),
        mkdirs,
        items,
        (e) => {
          if (!scope.current()) return;
          noteActivity();
          scope.uploadEvent(e);
          const progress = scope.bar(true);
          if (progress) getState().jobProgress(progress.fraction, progress.message);
        },
        () => stop(scope),
      );
      if (!scope.current()) return;
      const { uploads, base } = scope.uploadResult();
      const sent = samplesBar(uploads, base, false)?.sent ?? 0;
      const seconds = (Date.now() - started) / 1000;
      scope.setAverage(sent > 0 && seconds > 0 ? sent / seconds : undefined);
      // The row records the error, and a failed or stopped run keeps the source ticks for a retry.
      const failure = uploadFailure(uploads, base);
      if (failure) throw new Error(failure);
      const stopped = uploadCancelled(uploads, base);
      if (stopped) return { cancelled: stopped };
      done = true;
    });
    return done && scope.current();
  };

  /** A drop during a refresh waits briefly for the refresh to finish. */
  async function whenIdle(scope: Scope) {
    for (let i = 0; i < 200 && scope.current() && getState().busy() === 'Reading'; i++)
      await new Promise((resolve) => setTimeout(resolve, 50));
    return scope.current() && !getState().busy();
  }

  async function uploadRows(rows: LocalRow[] = source.picked(), folder?: string) {
    const scope = destination.capture();
    const write = scope?.write(folder);
    if (!scope || !write || !rows.length) return;
    const snapshot = source.snapshot();
    if (!(await whenIdle(scope)) || !write.current()) return;
    let plan;
    try {
      plan = await planUpload(rows, write.folder, snapshot.list);
    } catch (e) {
      if (write.current()) getState().setError(errorLine(e));
      return;
    }
    if (!write.current() || getState().busy()) return;
    const items = plan.files.map((f): UploadItem => ({
      folder: f.folder,
      name: f.name,
      size: f.size,
      read: () => snapshot.bytes(f.rel, f.name, f.added),
    }));
    if (await send(scope, write, plan.mkdirs, items)) snapshot.clearSelection();
  }

  async function uploadFiles(files: File[], folder: string) {
    const scope = destination.capture();
    const write = scope?.write(folder);
    if (!scope || !write || !files.length || !(await whenIdle(scope)) || !write.current()) return;
    await send(
      scope,
      write,
      [],
      files.map((f) => ({
        folder: write.folder,
        name: f.name,
        size: f.size,
        read: async () => new Uint8Array(await f.arrayBuffer()),
      })),
    );
  }

  function openDestination(folder: string) {
    if (writing(getState().busy())) return;
    const scope = destination.capture();
    if (!scope) return;
    scope.openFolder(folder);
    void run('Reading', async () => {});
  }

  function openLocation(kind?: SamplesKind) {
    if (writing(getState().busy())) return;
    const scope = destination.capture();
    if (!scope) return;
    scope.openLocation(kind);
    return run('Reading', async () => {});
  }

  function createFolder(name: string): boolean {
    const scope = destination.capture();
    const write = scope?.write();
    if (!scope || !write) return false;
    const why = folderNameError(name, scope.entries() ?? []);
    if (why) {
      getState().setError(why);
      return false;
    }
    const path = joinPath(write.folder, name);
    void run('Creating', async () => {
      const backend = await write.open('write');
      await backend.mkdir(path);
    });
    return true;
  }

  async function removeSelected() {
    const scope = destination.capture();
    const write = scope?.write();
    if (!scope || !write) return;
    const gone = deleteRows(scope.rows(), scope.selected());
    const folder = write.folder;
    await run('Deleting', async () => {
      const backend = await write.open('write');
      scope.check();
      scope.setNote('');
      let done = 0;
      let total = 0;
      try {
        for (const row of gone) {
          scope.check();
          if (row.kind === 'folder') {
            const res = await backend.removeTree(row.path, {
              onProgress: (d, t) => {
                if (!scope.current()) return;
                done = d;
                total = t;
                scope.setDeleting(deletingLine(d, t, gone.length > 1 ? row.name : undefined));
              },
              shouldStop: () => stop(scope),
            });
            if (!scope.current()) return;
            if (res.stopped) return { cancelled: `${row.name}: ${deletePartLine(res.removed, res.total)}` };
          } else {
            if (stop(scope)) return { cancelled: `${row.name} and later rows not deleted` };
            await backend.remove(folder, row.name);
            if (!scope.current()) return;
          }
          scope.forget(row.path);
          done = total = 0;
        }
      } catch (e) {
        if (!scope.current()) return;
        if (isCancelled(e)) return { cancelled: total ? deletePartLine(done, total) : 'stopped' };
        if (total) throw new Error(`${errorLine(e)} (${deletePartLine(done, total)})`, { cause: e });
        throw e;
      } finally {
        if (scope.current()) scope.setDeleting('');
      }
      if (scope.current()) scope.setNote(`Deleted ${gone.length} item${gone.length === 1 ? '' : 's'}.`);
    });
  }

  async function restart() {
    const scope = destination.capture();
    if (scope?.location()?.apply !== 'restart') return;
    await run('Restarting', async () => {
      const { asked } = getConnection();
      const session = await asked(scope.binding.session);
      scope.check();
      await asked(async () => {
        try {
          await restartDevice(session, scope.check);
        } catch (e) {
          scope.check();
          throw e;
        }
      });
    });
  }

  const transfer: Omit<SamplesSlot, 'Mode'> = {
    read,
    readInterrupted: () => activeRead !== undefined && !activeRead.scope?.current(),
    readLocal: source.read,
    localChanged: source.changed,
    lines: destination.lines,
    bar(busy) {
      const running = busy === 'Uploading';
      const info = destination.bar(running);
      return info ? { info, running } : undefined;
    },
  };

  const { snapshot: _snapshot, ...sourceView } = source;
  const { capture: _capture, ...destinationView } = destination;
  return {
    source: sourceView,
    destination: destinationView,
    transfer,
    uploadRows,
    uploadFiles,
    openDestination,
    openLocation,
    createFolder,
    removeSelected,
    restart,
  };
}

export type SamplesApp = ReturnType<typeof createSamplesApp>;
