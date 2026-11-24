import { batch, createSignal } from 'solid-js';
import type { SampleEntry, SamplesKind } from '@digi/dk-session/samples';
import { errorLine, isTimeout } from '@digi/workbench-core/modules/backend.ts';
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
import type { SampleDevice, SampleDeviceContext, SampleDeviceSession } from './device-session';

const deviceIdentity = ({ name, runtime }: SampleDevice) => ({ name, runtime, hasRuntime: runtime !== null });

export type SamplePageState = Pick<
  TransferApp['state'],
  'activeMode' | 'setDeviceInfo' | 'sampleLink' | 'setSampleLink' | 'busy' | 'setError' | 'stopWasAsked' | 'jobProgress'
>;

function createPresentation() {
  const [device, setDevice] = createSignal<SampleDevice>();
  const [generation, setGeneration] = createSignal(0);
  const [kind, setKind] = createSignal<SamplesKind>();
  const [folder, setFolder] = createSignal('');
  const [entries, setEntries] = createSignal<SampleEntry[]>();
  const [locationError, setLocationError] = createSignal('');
  const [selected, select] = createSignal<Set<string>>(new Set());
  const [uploads, setUploads] = createSignal<Uploads>({});
  const [base, setBase] = createSignal<Uploads>({});
  const [notes, setNotes] = createSignal<string[]>([]);
  const [average, setAverage] = createSignal<number>();
  const [note, setNote] = createSignal('');
  const [deleting, setDeleting] = createSignal('');
  return {
    device,
    setDevice,
    generation,
    setGeneration,
    kind,
    setKind,
    folder,
    setFolder,
    entries,
    setEntries,
    locationError,
    setLocationError,
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
  emulatorGeneration = () => 0,
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
      device,
      setDevice,
      generation,
      setGeneration,
      kind,
      setKind,
      folder,
      setFolder,
      entries,
      setEntries,
      locationError,
      setLocationError,
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
    const firmwareGeneration = () => (binding.target.kind === 'emulator' ? emulatorGeneration() : 0);
    const locations = () => (generation() === firmwareGeneration() ? device()?.locations : undefined);
    const location = () => locations()?.find((item) => item.kind === kind());
    const writable = () => {
      const selected = location();
      return !!selected && !selected.readOnly && !selected.unavailable && !!entries() && !locationError();
    };
    let readGeneration = 0;
    let navigation = 0;
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
      firmwareGeneration,
      link: () => state().sampleLink(),
      setLink: (sampleKind, link) => {
        if (current() && location()?.kind === sampleKind) state().setSampleLink(link);
      },
    });
    const unbind = binding.beforeLeave(async () => {
      released = true;
      unbind();
      setDeleting('');
      state().setSampleLink(undefined);
      await session.release();
    });

    function changeFolder(next: string) {
      batch(() => {
        setFolder(next);
        setEntries(undefined);
        setLocationError('');
        select(new Set<string>());
        setNote('');
      });
    }

    function changeLocation(next: SamplesKind | undefined) {
      batch(() => {
        setKind(next);
        changeFolder('');
        setUploads({});
        setBase({});
        setNotes([]);
        setAverage(undefined);
        const found = device();
        state().setDeviceInfo(found && deviceIdentity(found));
        state().setSampleLink(undefined);
      });
    }

    function openFolder(next: string) {
      if (!location() || next === folder()) return;
      navigation++;
      readGeneration++;
      changeFolder(next);
    }

    function openLocation(next?: SamplesKind) {
      if (next === kind() || (next !== undefined && !locations()?.some((item) => item.kind === next))) return;
      if (next === undefined && (locations()?.length ?? 0) < 2) return;
      navigation++;
      readGeneration++;
      changeLocation(next);
    }

    async function read() {
      const read = ++readGeneration;
      const image = firmwareGeneration();
      const showing = () =>
        current() && read === readGeneration && image === firmwareGeneration() && state().activeMode() === 'samples';
      try {
        const found = await session.identify();
        if (!showing()) return;
        batch(() => {
          if (generation() !== image) changeLocation(undefined);
          setGeneration(image);
          setDevice(found);
          const next = found.locations.length === 1 ? found.locations[0]?.kind : location()?.kind;
          if (next !== kind()) changeLocation(next);
        });
        const identity = deviceIdentity(found);
        const selected = location();
        if (!selected || selected.unavailable) {
          state().setDeviceInfo(identity);
          return;
        }
        const path = folder();
        const { kind: samples, apply } = selected;
        const backend = await session.open(samples, 'read');
        if (!showing()) return;
        const canMkdir = await backend.canMkdir();
        if (!showing()) return;
        const listed = await backend.list(path);
        if (!showing()) return;
        let free: number | undefined;
        try {
          free = (await backend.space()).free;
        } catch {
          // The listing can still be used when the target does not report free space.
        }
        if (showing()) {
          batch(() => {
            setEntries(listed);
            setLocationError('');
            state().setDeviceInfo({ ...identity, samples, canMkdir, free, apply });
          });
        }
      } catch (e) {
        if (!showing()) return;
        if ((locations()?.length ?? 0) < 2) throw e;
        setLocationError(errorLine(e));
      }
    }

    function write(path = folder()) {
      const connected = device();
      const selected = location();
      if (!connected || !selected || !writable()) return undefined;
      const version = navigation;
      const image = firmwareGeneration();
      const stillHere = () =>
        current() && navigation === version && image === firmwareGeneration() && kind() === selected.kind;
      return {
        current: stillHere,
        folder: path,
        name: connected.name,
        async open(purpose: 'write' | 'upload') {
          if (!stillHere()) throw new Error('The sample destination changed.');
          const backend = await session.open(selected.kind, purpose).catch((error) => {
            // A timed-out write preflight skips the normal readback. Invalidate the writable view here.
            if (stillHere() && isTimeout(error)) {
              batch(() => {
                setEntries(undefined);
                state().setDeviceInfo(deviceIdentity(connected));
              });
            }
            throw error;
          });
          if (!stillHere()) throw new Error('The sample destination changed.');
          return backend;
        },
      };
    }

    return {
      binding,
      presentation,
      current,
      check,
      locations,
      location,
      locationError,
      writable,
      folder,
      entries,
      rows: () => sampleRows(folder(), entries() ?? [], uploads()),
      selected,
      select,
      openFolder,
      openLocation,
      read,
      write,
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
    locations: () => visible()?.locations(),
    location: () => visible()?.location(),
    locationError: () => visible()?.locationError() ?? '',
    writable: () => !target().switching() && (visible()?.writable() ?? false),
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
