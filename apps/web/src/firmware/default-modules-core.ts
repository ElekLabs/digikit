// Default modules: run-time modules the desktop workbench installs and enables together with the firmware (devices/*.toml `[digikit] default_modules`).
// The pure part, with no DOM and no Tauri: the user's unticked choices and the install run over a few operations (tested with fakes). The run:
// wait until the runtime answers, upload and verify each ticked module, then change the selection and restart once when it must change: the
// ticked modules that are not loaded yet are added, and the unticked default modules that are loaded are removed (what the runtime loaded
// before and is no default module stays selected). The selection persists (the emulator keeps it in the card folder), so unticking has to
// remove. A failure names the module; the firmware flash itself is not undone.

/** What the list shows of a default module. */
export interface DefaultModuleEntry {
  /** The module id: `digikit/remote`. */
  id: string;
  /** One line, from the module's own `module.toml` (`summary`). */
  summary: string;
}

/** The ids the user unticked, per device id; everything else is ticked. */
export type Unticked = Record<string, string[]>;

export function parseUnticked(json: string | null): Unticked {
  try {
    const v = JSON.parse(json ?? '{}') as unknown;
    if (!v || typeof v !== 'object' || Array.isArray(v)) return {};
    const out: Unticked = {};
    for (const [device, ids] of Object.entries(v))
      if (Array.isArray(ids) && ids.every((id) => typeof id === 'string')) out[device] = ids as string[];
    return out;
  } catch {
    return {};
  }
}

export function withTick(unticked: Unticked, device: string, id: string, on: boolean): Unticked {
  const rest = (unticked[device] ?? []).filter((x) => x !== id);
  return { ...unticked, [device]: on ? rest : [...rest, id] };
}

/** The ids of `entries` that are ticked for the device, in list order. */
export const tickedIds = (entries: DefaultModuleEntry[], unticked: Unticked, device: string): string[] =>
  entries.filter((e) => !(unticked[device] ?? []).includes(e.id)).map((e) => e.id);

/** The device file's patch set the Firmware page asks for: `device`, or with the Custom boot screen toggle the same roots plus core/introblank. */
export const patchSetFor = (customBootScreen: boolean): string => (customBootScreen ? 'device-introblank' : 'device');

/** What the toggle says under its label. */
export const CUSTOM_BOOT_SCREEN_HELP =
  'Replaces the Elektron intro with black until a boot screen module takes over. Needs the Boot screens module.';

/** The ids of `entries` that are unticked for the device: the default modules the user turned off (the only modules a run may remove). */
export const untickedIds = (entries: DefaultModuleEntry[], unticked: Unticked, device: string): string[] =>
  entries.filter((e) => (unticked[device] ?? []).includes(e.id)).map((e) => e.id);

/** The text of the Default modules button. */
export const defaultsLabel = (ticked: number): string => `Default modules (${ticked})`;

/** A default module failed: `id` is the module, `message` the reason. */
export class DefaultModuleError extends Error {
  readonly id: string;
  constructor(id: string, reason: string) {
    super(`${id}: ${reason}`);
    this.id = id;
    this.name = 'DefaultModuleError';
  }
}

/** What the run needs of a device (a protocol session with the library of built modules, or a fake). */
export interface DefaultOps {
  /** The store path of the module's built `.dkm` for this device, or undefined when there is none. */
  pathOf(id: string): string | undefined;
  /** Resolves once the runtime answers INFO; throws when it does not in time. */
  waitForRuntime(): Promise<void>;
  /** Upload and verify one module; `same` when the card already had the same file. */
  upload(id: string): Promise<'uploaded' | 'same'>;
  /** The ids of the modules loaded now. */
  loaded(): Promise<string[]>;
  /** Save the selection (what the next boot loads). */
  select(paths: string[]): Promise<void>;
  /** Restart and wait for the boot; the ids loaded after it. */
  restart(): Promise<string[]>;
}

export interface DefaultProgress {
  text: string;
  /** Steps done of `total`. */
  done: number;
  total: number;
}

/** The selection holds at most this many paths (SELECT). */
export const SELECTION_MAX = 8;

const reason = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/**
 * Install and enable `ids`, and disable the loaded modules of `drop` (the unticked default modules; no other module is ever removed).
 * Returns one line for each module and for the change; throws `DefaultModuleError` for the first that fails.
 */
export async function installDefaults(
  ops: DefaultOps,
  ids: string[],
  progress: (p: DefaultProgress) => void = () => {},
  drop: string[] = [],
): Promise<string[]> {
  if (ids.length === 0 && drop.length === 0) return [];
  const lines: string[] = [];
  const first = ids[0] ?? drop[0]!;
  const total = ids.length + 2;
  const wrap = async <T>(id: string, step: Promise<T>): Promise<T> => {
    try {
      return await step;
    } catch (e) {
      throw e instanceof DefaultModuleError ? e : new DefaultModuleError(id, reason(e));
    }
  };
  for (const id of ids)
    if (!ops.pathOf(id))
      throw new DefaultModuleError(id, 'no built .dkm for this device (build it: mise run module:pack-all)');
  progress({ text: 'Waiting for the runtime to answer', done: 0, total });
  await wrap(first, ops.waitForRuntime());
  for (const [i, id] of ids.entries()) {
    progress({ text: `Installing ${id} (${i + 1} of ${ids.length})`, done: i + 1, total });
    lines.push(`${id}: ${(await wrap(id, ops.upload(id))) === 'same' ? 'already on the card' : 'uploaded'}`);
  }
  const loaded = await wrap(first, ops.loaded());
  const missing = ids.filter((id) => !loaded.includes(id));
  const extra = drop.filter((id) => loaded.includes(id) && !ids.includes(id));
  if (missing.length === 0 && extra.length === 0) {
    lines.push('already enabled');
    return lines;
  }
  const keep = loaded.filter((id) => !extra.includes(id));
  const paths = [...new Set([...keep, ...ids].map((id) => ops.pathOf(id)).filter((p): p is string => !!p))];
  if (paths.length > SELECTION_MAX)
    throw new DefaultModuleError(
      missing[0] ?? extra[0]!,
      `the selection would hold ${paths.length} modules, the limit is ${SELECTION_MAX}`,
    );
  const change = missing[0] ?? extra[0]!;
  progress({
    text: `${missing.length ? `Enabling ${missing.join(', ')}` : `Disabling ${extra.join(', ')}`}: restarting the runtime`,
    done: ids.length + 1,
    total,
  });
  await wrap(change, ops.select(paths));
  const after = await wrap(change, ops.restart());
  const still = missing.filter((id) => !after.includes(id));
  if (still.length)
    throw new DefaultModuleError(still[0]!, 'it is not loaded after the restart (see the Log for the reason)');
  const stays = extra.filter((id) => after.includes(id));
  if (stays.length)
    throw new DefaultModuleError(stays[0]!, 'it is still loaded after the restart (see the Log for the reason)');
  lines.push(
    [missing.length ? `enabled ${missing.join(', ')}` : '', extra.length ? `disabled ${extra.join(', ')}` : '']
      .filter(Boolean)
      .join(', ') + ' (restarted once)',
  );
  progress({ text: 'Default modules installed', done: total, total });
  return lines;
}
