import { createSignal } from 'solid-js';
import type { SampleEntry } from '@digi/dk-session/samples';
import type { TransferApp } from '@digi/workbench-core/transfer/app.ts';
import type { TargetApp } from '@digi/workbench-core/transfer/target.ts';
import {
  applyUpload,
  dropUploads,
  uploadLines,
  type UploadEvent,
  type Uploads,
} from '@digi/workbench-core/transfer/sample-upload.ts';
import { sampleRows, samplesBar } from '@digi/workbench-core/transfer/samples-core.ts';
import type { SampleDeviceContext, SampleDeviceSession } from './device-session';

export type SamplePageState = Pick<
  TransferApp['state'],
  'activeMode' | 'setDeviceInfo' | 'sampleLink' | 'setSampleLink' | 'busy' | 'setError' | 'stopWasAsked' | 'jobProgress'
>;

function createPresentation() {
  const [folder, setFolder] = createSignal('');
  const [entries, setEntries] = createSignal<SampleEntry[]>();
  const [selected, select] = createSignal<Set<string>>(new Set());
  const [uploads, setUploads] = createSignal<Uploads>({});
  const [base, setBase] = createSignal<Uploads>({});
  const [notes, setNotes] = createSignal<string[]>([]);
  const [average, setAverage] = createSignal<number>();
  const [note, setNote] = createSignal('');
  const [deleting, setDeleting] = createSignal('');
  return {
    folder,
    setFolder,
    entries,
    setEntries,
    selected,
    select,
    uploads,
    setUploads,
    base,
    setBase,
    notes,
    setNotes,
    average,
    setAverage,
    note,
    setNote,
    deleting,
    setDeleting,
  };
}

export function createSamplesDestination(
  target: () => Pick<TargetApp, 'target' | 'switching' | 'bindSession'>,
  state: () => SamplePageState,
  makeSession: (context: SampleDeviceContext) => SampleDeviceSession,
) {
  const emptyTicks = new Set<string>();
  const [scope, setScope] = createSignal<ReturnType<typeof bind>>();
  const visible = () => {
    const selection = target().target();
    const held = scope();
    return held?.binding.target === selection ? held : undefined;
  };

  function bind(presentation = createPresentation()) {
    const binding = target().bindSession();
    const {
      folder,
      setFolder,
      entries,
      setEntries,
      selected,
      select,
      uploads,
      setUploads,
      base,
      setBase,
      notes,
      setNotes,
      average,
      setAverage,
      note,
      setNote,
      deleting,
      setDeleting,
    } = presentation;
    let readGeneration = 0;
    let released = false;
    const check = () => {
      if (released) throw new Error('The selected target changed.');
      binding.check();
    };
    const current = () => {
      try {
        check();
        return true;
      } catch {
        return false;
      }
    };
    const session = makeSession({
      binding,
      current,
      check,
      setLink: (link) => {
        if (current()) state().setSampleLink(link);
      },
    });
    const unbind = binding.beforeLeave(async () => {
      released = true;
      unbind();
      setDeleting('');
      state().setSampleLink(undefined);
      await session.release();
    });

    function open(next: string) {
      readGeneration++;
      setFolder(next);
      select(new Set<string>());
      setNote('');
    }

    async function read() {
      const generation = ++readGeneration;
      const path = folder();
      const showing = () => current() && generation === readGeneration && state().activeMode() === 'samples';
      try {
        const { name, runtime, hasRuntime, samples, apply, backend } = await session.open('read');
        if (!showing()) return;
        const canMkdir = await backend.canMkdir();
        if (!showing()) return;
        const listed = await backend.list(path);
        if (!showing()) return;
        setEntries(listed);
        let free: number | undefined;
        try {
          free = (await backend.space()).free;
        } catch {
          // The listing can still be used when the target does not report free space.
        }
        if (showing()) state().setDeviceInfo({ name, runtime, hasRuntime, samples, canMkdir, free, apply });
      } catch (e) {
        if (showing()) throw e;
      }
    }

    return {
      binding,
      presentation,
      current,
      check,
      folder,
      entries,
      rows: () => sampleRows(folder(), entries() ?? [], uploads()),
      selected,
      select,
      open,
      read,
      openSession: session.open,
      note,
      setNote,
      deleting,
      setDeleting,
      forget(path: string) {
        setUploads((old) => dropUploads(old, [path]));
        select(new Set([...selected()].filter((key) => key !== path)));
      },
      beginUpload() {
        setNote('');
        setBase(uploads());
        setNotes([]);
        setAverage(undefined);
      },
      uploadEvent(event: UploadEvent) {
        if (event.event === 'note') setNotes((old) => [...old, event.message]);
        setUploads((old) => applyUpload(old, event));
      },
      uploadResult: () => ({ uploads: uploads(), base: base() }),
      setAverage,
      bar: (running: boolean) => samplesBar(uploads(), base(), running, average(), state().sampleLink()),
      lines: () => [...notes(), ...uploadLines(uploads(), base())],
    };
  }

  function capture() {
    if (target().switching()) return undefined;
    let held = visible();
    if (!held?.current()) {
      held = bind(held?.presentation);
      setScope(held);
    }
    return held;
  }

  return {
    capture,
    folder: () => visible()?.folder() ?? '',
    entries: () => visible()?.entries(),
    rows: () => visible()?.rows() ?? [],
    selected: () => visible()?.selected() ?? emptyTicks,
    select: (next: Set<string>) => capture()?.select(next),
    note: () => visible()?.note() ?? '',
    deleting: () => visible()?.deleting() ?? '',
    bar: (running: boolean) => visible()?.bar(running),
    lines: () => visible()?.lines() ?? [],
  };
}
