import { batch, createSignal } from 'solid-js';
import type { DspReportSource, SavedSession, SessionDocument, SessionHost } from '@digi/workbench-core/host/session.ts';
import type { KeyValue } from '@digi/workbench-core/storage.ts';
import type { SessionBoot } from './emulator/firmware';

const UNTITLED = 'Untitled Session';
const RECENTS = 'digikit.sessions.recent';
type Recent = { root: string; name: string };

function readRecent(storage: KeyValue): Recent[] {
  try {
    const items: unknown = JSON.parse(storage.get(RECENTS) ?? '[]');
    return Array.isArray(items) ? items.filter((v) => typeof v?.root === 'string' && typeof v?.name === 'string') : [];
  } catch {
    return [];
  }
}

export function createWorkspaceSession(options: {
  host: SessionHost;
  pickFirmware(start: string | null): Promise<string | null>;
  firmware: {
    source(): { path: string } | undefined;
    emulatorLoaded(): boolean;
    bootRuntime(): boolean;
    bootDsp(): boolean;
    bootSession(pin: SessionBoot): Promise<boolean>;
    updateSessionBuild(imageSha256: string | null): void;
    closeSession(): Promise<void>;
  };
  storage: KeyValue;
  selectEmulator(): Promise<void>;
  confirm(message: string): boolean;
  blocked(): boolean;
  openCapture(file: Pick<File, 'name' | 'text'>): Promise<void>;
  clearCapture(): void;
}) {
  const { host, firmware, storage } = options;
  const [saved, setSaved] = createSignal<SavedSession>();
  const [dspSession, setDspSession] = createSignal<SavedSession>();
  const [card, setCard] = createSignal<string>();
  const [name, setName] = createSignal(UNTITLED);
  const [recent, setRecent] = createSignal(readRecent(storage));
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal('');
  const [note, setNote] = createSignal('');
  const [draft, setDraft] = createSignal(false);
  const disabled = () => busy() || options.blocked();
  const dirty = () => !!card() || name() !== (saved()?.document.name ?? UNTITLED);
  const mayDiscard = () =>
    (!card() && !dirty()) || options.confirm('Close this session and discard any changes since the last checkpoint?');

  function accept(session: SavedSession, switched = false) {
    const { root, document } = session;
    const updated = [{ root, name: document.name }, ...recent().filter((r) => r.root !== root)].slice(0, 8);
    batch(() => {
      setSaved(session);
      if (switched) setDspSession(session);
      setName(document.name);
      setRecent(updated);
      setDraft(false);
    });
    storage.set(RECENTS, JSON.stringify(updated));
  }

  async function run(task: () => Promise<void>) {
    if (disabled()) return;
    setBusy(true);
    setError('');
    setNote('');
    try {
      await task();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function detach() {
    const mount = card();
    if (mount) await firmware.closeSession();
    setCard(undefined);
    if (mount) await host.release(mount);
    options.clearCapture();
  }

  async function open(root?: string) {
    if (!mayDiscard()) return;
    await run(async () => {
      const path = root ?? (await host.pick('open'));
      if (!path) return;
      const session = await host.open(path);
      await detach();
      accept(session, true);
    });
  }

  async function start(session: SavedSession, replacement?: string) {
    const [next, mount] = await host.mount(session.root, replacement);
    const pin = next.document.firmware;
    try {
      await options.selectEmulator();
      if (
        !(await firmware.bootSession({
          path: pin.path,
          card: mount,
          runtime: pin.runtime,
          dsp: pin.dsp,
          sourceSha256: pin.sha256,
          imageSha256: pin.imageSha256 ?? undefined,
          preserve: () => host.preserve(mount),
        }))
      )
        throw new Error('Could not start the session. See the emulator or Log for details.');
    } catch (error) {
      await host.release(mount).catch(() => {});
      throw error;
    }
    accept(next);
    setCard(mount);
    setNote('Working card mounted. Save to write a checkpoint.');
  }

  async function saveAs() {
    await run(async () => {
      const session = saved();
      const file = firmware.source()?.path;
      if (!session && (!file || !firmware.emulatorLoaded())) {
        setDraft(true);
        setNote('Boot firmware with its default card, or choose a card for a new session.');
        return;
      }
      const copyName = session ? `${name()} Copy` : name();
      const destination = await host.pick('save', copyName);
      if (!destination) return;
      const created = await host.create({
        destination,
        name: copyName,
        firmware: file ?? '',
        source: 'live',
        card: card(),
        fork: session?.root,
        runtime: firmware.bootRuntime(),
        dsp: firmware.bootDsp(),
      });
      // A live fork has the same immutable base and the same working writes.
      // Keep playing. All subsequent saves go to the copy, not the original.
      accept(created, true);
      if (card()) firmware.updateSessionBuild(created.document.firmware.imageSha256);
      setNote(
        card()
          ? 'Working card now saves to this copy. Previous captures stayed in the original.'
          : 'Saved. Start the session to switch from the default card to the saved copy.',
      );
    });
  }

  return {
    saved,
    dspSession,
    card,
    name,
    setName,
    recent,
    busy,
    error,
    note,
    draft,
    dirty,
    disabled,
    open,
    saveAs,
    captureContext: () => {
      const session = saved()?.document;
      return card() && session
        ? { id: session.id, saved_checkpoint: session.checkpoint, working_card: true }
        : undefined;
    },
    newSession: () => {
      if (!mayDiscard()) return;
      return run(async () => {
        await detach();
        setSaved(undefined);
        setDspSession(undefined);
        setName(UNTITLED);
        setDraft(true);
      });
    },
    create: (source: 'image' | 'folder') =>
      run(async () => {
        const path = await options.pickFirmware(firmware.source()?.path ?? null);
        if (!path) return;
        const input = await host.pick(source);
        if (!input) return;
        const destination = await host.pick('save', name());
        if (!destination) return;
        const next = await host.create({
          destination,
          name: name(),
          source,
          firmware: path,
          card: input,
          runtime: firmware.bootRuntime(),
          dsp: firmware.bootDsp(),
        });
        accept(next, true);
        setNote('Saved. Start the session when you are ready.');
      }),
    save: () => {
      const session = saved();
      if (!session) return saveAs();
      return run(async () => {
        const next = await host.save(session.root, card(), name());
        accept(next);
        if (card()) firmware.updateSessionBuild(next.document.firmware.imageSha256);
        setNote(card() ? 'Working card checkpoint saved.' : 'Session saved.');
      });
    },
    saveDsp: async (module: string, source?: DspReportSource) => {
      const root = saved()?.root;
      if (!root || disabled()) return;
      await run(async () => {
        const next = await host.saveDsp(root, module, source);
        setSaved(next);
        setNote(source ? 'DSP report saved.' : 'DSP selection saved.');
      });
      const failure = error();
      if (failure) throw new Error(failure);
    },
    start: () =>
      run(async () => {
        const session = saved();
        if (session && !card()) await start(session);
      }),
    locate: () =>
      run(async () => {
        const session = saved();
        if (!session || card()) return;
        const path = await options.pickFirmware(session.document.firmware.path);
        if (path) await start(session, path);
      }),
    revert: () => {
      const session = saved();
      if (
        !session ||
        !card() ||
        !options.confirm('Discard the working card changes and reload the last saved checkpoint?')
      )
        return;
      return run(async () => {
        await detach();
        await start(session);
      });
    },
    close: () => {
      if (!mayDiscard()) return;
      return run(async () => {
        await detach();
        setSaved(undefined);
        setDspSession(undefined);
        setName(UNTITLED);
        setDraft(false);
      });
    },
    attach: () =>
      run(async () => {
        const session = saved();
        if (!session) return;
        const path = await host.pick('attachment');
        if (path) {
          const next = await host.attach(session.root, path);
          setSaved(next);
          setNote('Attachment copied into the session folder.');
        }
      }),
    openAttachment: (item: SessionDocument['attachments'][number]) =>
      run(async () => {
        const session = saved();
        if (!session) return;
        if (item.kind === 'capture') {
          await options.openCapture({ name: item.name, text: () => host.readCapture(session.root, item.path) });
        } else await host.openAttachment(session.root, item.path);
      }),
  };
}
