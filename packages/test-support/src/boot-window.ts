import fs from 'node:fs';
import path from 'node:path';
import { parse as parseToml } from 'smol-toml';
import { repoRoot, type Device } from './firmware-inputs.ts';
import type { Clock } from './emulated-clock.ts';

/**
 * The boot window (docs/TESTING.md "The boot window"): a device whose firmware ends its boot with a ConfirmWindow (a title and "PRESS Y/N TO CLOSE") has it on top of the view stack.
 * It takes every panel key until NO or YES answers it. This step reads the firmware's view stack, waits for the main screen, waits while the filesystem check that makes the window is pending or runs, and answers the window with NO.
 * It uses only firmware state and the clock of the target, so one step serves the emulator and (through `DkSession.peek`) a device. The facts are the `[ui]` tables of `devices/<profile>.toml`.
 */

/** The `[ui]` and `[ui.boot_window]` facts of a device file; offsets in bytes, addresses are ColdFire addresses. */
/** What reading the view stack needs: the Brain, the view manager and the list nodes (`[ui]` of the device file). */
export interface ViewStackFacts {
  /** The global that holds the Brain pointer (0 until the Brain exists). */
  brainPtr: number;
  /** The view manager is the object at Brain + this. */
  viewManagerOff: number;
  /** Manager + this is the sentinel node of the view list; the top view is the node before it, the bottom view the node after it. */
  listOff: number;
  /** The offset of the next pointer in a list node (the sentinel included). */
  listNextOff: number;
  /** The offset of the previous pointer in a list node (the sentinel included). */
  listPrevOff: number;
  /** The offset of the View pointer in a list node. */
  nodeViewOff: number;
}

export interface BootWindowFacts extends ViewStackFacts {
  /** The title of the window that ends the boot. */
  title: string;
  /** The vtable pointer stored at +0 of the MainScreenView: the main screen is reached once the bottom view of the list is this one (overlays such as the help bubble sit above it). */
  mainVptr: number;
  /** The vtable pointer stored at +0 of a ConfirmWindow, and the offsets of its result (0 unanswered, 1 YES, 2 NO) and of the data pointer of its title string. */
  confirmVptr: number;
  confirmResultOff: number;
  confirmTitleOff: number;
  /** The vtable pointer stored at +0 of a ProgressWindow (the view of a running filesystem check). */
  progressVptr: number;
  /**
   * What decides whether the main screen starts the filesystem check (the firmware's test 0x4001058c on DT2 1.17): the check is pending while the u32 at `fsMountedPtr` is nonzero and
   * the u32 at `fsStatePtr` is `fsStateCheck` or the u32 at `fsCheckedPtr` is 0. These words are fields of the card's filesystem superblock, read at mount: the check runs only for an unverified
   * superblock, and a successful check writes state 4 and checked 1 back to it, so the test is false afterwards.
   */
  fsMountedPtr: number;
  fsStatePtr: number;
  fsStateCheck: number;
  fsCheckedPtr: number;
}

/** What is on top of the view stack. */
export type TopView =
  /** No Brain, no manager or an empty view list. */
  | { kind: 'none' }
  /** A read failed (an unmapped address, a pointer that is not set yet while the list changes): read again at the next poll. */
  | { kind: 'unknown'; why: string }
  /** The main screen. */
  | { kind: 'main' }
  /** A ProgressWindow: the check that ends in the boot window is running. */
  | { kind: 'progress' }
  /** The boot window; `result` 0 is unanswered. */
  | { kind: 'boot-window'; result: number }
  /** Any other view (an overlay, another window), with its vtable pointer; for a ConfirmWindow with another title, also that title. */
  | { kind: 'other'; vptr: number; title?: string };

/** `length` bytes of guest memory from `addr` (the emulator: `mem.cf.peek`; a device: `DkSession.peek`). */
export type ReadMemory = (addr: number, length: number) => Promise<Uint8Array>;

export interface BootWindowTarget {
  read: ReadMemory;
  ui: { tap(key: 'NO'): Promise<void> };
  /** Emulated time on the emulator. */
  clock: Pick<Clock, 'now' | 'pause'>;
}

const num = (table: Record<string, unknown>, key: string, where: string): number => {
  const v = table[key];
  if (typeof v !== 'number' || !Number.isInteger(v) || v < 0)
    throw new Error(`${where}: ${key} must be a non-negative integer`);
  return v;
};

/** The view stack facts from the parsed device file's `[ui]` table; undefined when it has no `brain_ptr` (the device file does not give them). */
export function viewStackFactsOf(
  file: { ui?: Record<string, unknown> },
  where = 'device file',
): ViewStackFacts | undefined {
  const ui = file.ui;
  if (!ui || ui.brain_ptr === undefined) return undefined;
  const u = `${where} [ui]`;
  return {
    brainPtr: num(ui, 'brain_ptr', u),
    viewManagerOff: num(ui, 'view_manager_off', u),
    listOff: num(ui, 'list_off', u),
    listNextOff: num(ui, 'list_next_off', u),
    listPrevOff: num(ui, 'list_prev_off', u),
    nodeViewOff: num(ui, 'node_view_off', u),
  };
}

/** The facts from the parsed device file; undefined when it has no `[ui.boot_window]` table (the device has no boot window). */
export function bootWindowFactsOf(
  file: { ui?: Record<string, unknown> },
  where = 'device file',
): BootWindowFacts | undefined {
  const ui = file.ui;
  const window = ui?.boot_window as Record<string, unknown> | undefined;
  if (!ui || !window) return undefined;
  if (typeof window.title !== 'string' || window.title === '')
    throw new Error(`${where}: [ui.boot_window] title must be a string`);
  const w = `${where} [ui.boot_window]`;
  const stack = viewStackFactsOf(file, where);
  if (!stack) throw new Error(`${where} [ui]: brain_ptr is missing`);
  return {
    ...stack,
    title: window.title,
    mainVptr: num(window, 'main_vptr', w),
    confirmVptr: num(window, 'confirm_vptr', w),
    confirmResultOff: num(window, 'confirm_result_off', w),
    confirmTitleOff: num(window, 'confirm_title_off', w),
    progressVptr: num(window, 'progress_vptr', w),
    fsMountedPtr: num(window, 'fs_mounted_ptr', w),
    fsStatePtr: num(window, 'fs_state_ptr', w),
    fsStateCheck: num(window, 'fs_state_check', w),
    fsCheckedPtr: num(window, 'fs_checked_ptr', w),
  };
}

/** The parsed device file of `device` (found by `[device] short`). */
function deviceFile(device: Device): { name: string; file: { ui?: Record<string, unknown> } } {
  const dir = path.join(repoRoot(), 'devices');
  for (const name of fs.readdirSync(dir).filter((n) => n.endsWith('.toml'))) {
    const file = parseToml(fs.readFileSync(path.join(dir, name), 'utf8')) as {
      device?: { short?: string };
      ui?: Record<string, unknown>;
    };
    if (file.device?.short === device) return { name: `devices/${name}`, file };
  }
  throw new Error(`no device file for ${device}`);
}

const stackCache = new Map<Device, ViewStackFacts | undefined>();
const factsCache = new Map<Device, BootWindowFacts | undefined>();

/** The view stack facts of `device`, read from its `devices/*.toml`; undefined: the device file does not give them. No boot window is needed. */
export function viewStackFacts(device: Device): ViewStackFacts | undefined {
  if (stackCache.has(device)) return stackCache.get(device);
  const { name, file } = deviceFile(device);
  const facts = viewStackFactsOf(file, name);
  stackCache.set(device, facts);
  return facts;
}

/** The boot window facts of `device`, read from its `devices/*.toml`; undefined: the device has no boot window. */
export function bootWindowFacts(device: Device): BootWindowFacts | undefined {
  if (factsCache.has(device)) return factsCache.get(device);
  const { name, file } = deviceFile(device);
  const facts = bootWindowFactsOf(file, name);
  factsCache.set(device, facts);
  return facts;
}

const u32At = (b: Uint8Array): number => new DataView(b.buffer, b.byteOffset, b.byteLength).getUint32(0);

/** Classify a top view from its vtable pointer and, for a ConfirmWindow, its result and the bytes at its title pointer (at least `title.length + 1` of them). Pure. */
export function classifyTopView(
  facts: BootWindowFacts,
  vptr: number,
  confirm?: { result: number; titleBytes: Uint8Array },
): TopView {
  if (vptr === facts.progressVptr) return { kind: 'progress' };
  if (vptr === facts.mainVptr) return { kind: 'main' };
  if (vptr === facts.confirmVptr && confirm) {
    const want = facts.title;
    const got = confirm.titleBytes;
    const same =
      got.length > want.length && got[want.length] === 0 && [...want].every((c, i) => got[i] === c.charCodeAt(0));
    if (same) return { kind: 'boot-window', result: confirm.result };
    const end = got.indexOf(0);
    const title = String.fromCharCode(...(end < 0 ? got : got.subarray(0, end)));
    return { kind: 'other', vptr, title };
  }
  return { kind: 'other', vptr };
}

/** Read the top view of the firmware's view stack and classify it. A read that fails or finds a pointer that is not set yet gives `unknown`: the caller reads again. */
export async function readTopView(read: ReadMemory, facts: BootWindowFacts): Promise<TopView> {
  try {
    const word = async (addr: number) => u32At(await read(addr, 4));
    const brain = await word(facts.brainPtr);
    if (brain === 0) return { kind: 'none' };
    // the view manager is an object inside the Brain, not a pointer
    const list = brain + facts.viewManagerOff + facts.listOff;
    const top = await word(list + facts.listPrevOff);
    // an empty list: the sentinel points to itself
    if (top === list) return { kind: 'none' };
    const view = await word(top + facts.nodeViewOff);
    if (view === 0) return { kind: 'none' };
    const vptr = await word(view);
    if (vptr !== facts.confirmVptr) return classifyTopView(facts, vptr);
    const result = await word(view + facts.confirmResultOff);
    const title = await word(view + facts.confirmTitleOff);
    if (title === 0) return { kind: 'unknown', why: 'the title pointer of a ConfirmWindow is not set' };
    return classifyTopView(facts, vptr, {
      result,
      titleBytes: await read(title, Math.max(facts.title.length + 1, 40)),
    });
  } catch (error) {
    return { kind: 'unknown', why: error instanceof Error ? error.message : String(error) };
  }
}

/** The opaque byte of a View (`View+0x16`, `View::vfunc_9` reads it; a full-screen view is 1, an overlay 0). */
export const VIEW_OPAQUE_OFF = 0x16;

/** One view of the firmware's view stack. */
export interface StackView {
  /** The View object. */
  view: number;
  /** Its vtable pointer: the class. */
  vptr: number;
  /** Its opaque byte: nonzero hides the views below it. */
  opaque: number;
}

/** The firmware's view stack, the top view first. Empty before the Brain exists. */
export async function readViewStack(read: ReadMemory, facts: ViewStackFacts, max = 32): Promise<StackView[]> {
  const word = async (addr: number) => u32At(await read(addr, 4));
  const brain = await word(facts.brainPtr);
  if (brain === 0) return [];
  const list = brain + facts.viewManagerOff + facts.listOff;
  const out: StackView[] = [];
  let node = await word(list + facts.listPrevOff);
  while (node !== list && out.length < max) {
    const view = await word(node + facts.nodeViewOff);
    out.push({ view, vptr: await word(view), opaque: (await read(view + VIEW_OPAQUE_OFF, 1))[0]! });
    node = await word(node + facts.listPrevOff);
  }
  return out;
}

/** The size of one item of a stock menu's item vector: a shared_ptr<MenuItem>. */
const MENU_ITEM_BYTES = 8;

/** The offset of field `field` in the struct `struct` of ports/fw/menu.toml. */
function menuField(struct: string, field: string): number {
  const rules = parseToml(fs.readFileSync(path.join(repoRoot(), 'ports/fw/menu.toml'), 'utf8')) as {
    struct: { name: string; fields: { name: string; off: number }[] }[];
  };
  const f = rules.struct.find((x) => x.name === struct)?.fields.find((x) => x.name === field);
  if (!f) throw new Error(`ports/fw/menu.toml: no ${struct}.${field}`);
  return f.off;
}

/** How many items the menu of the top view has (the item vector of the stock Menu object). The top view must be a stock menu, such as SETTINGS; throws on an empty stack. */
export async function readTopMenuItems(read: ReadMemory, facts: ViewStackFacts): Promise<number> {
  const [top] = await readViewStack(read, facts);
  if (!top) throw new Error('the view stack is empty');
  const word = async (addr: number) => u32At(await read(addr, 4));
  const menu = top.view - menuField('cf.menu.struct.VerticalMenuView', 'view');
  const begin = await word(menu + menuField('cf.menu.struct.CompactVerticalMenuView', 'items_begin'));
  const end = await word(menu + menuField('cf.menu.struct.CompactVerticalMenuView', 'items_end'));
  return (end - begin) / MENU_ITEM_BYTES;
}

/** Is the bottom view of the firmware's view stack the MainScreenView (the main screen is reached; overlays may sit above it)? undefined when a read fails: read again. */
export async function readMainAtBottom(read: ReadMemory, facts: BootWindowFacts): Promise<boolean | undefined> {
  try {
    const word = async (addr: number) => u32At(await read(addr, 4));
    const brain = await word(facts.brainPtr);
    if (brain === 0) return false;
    const list = brain + facts.viewManagerOff + facts.listOff;
    const first = await word(list + facts.listNextOff);
    if (first === list) return false;
    const view = await word(first + facts.nodeViewOff);
    if (view === 0) return false;
    return (await word(view)) === facts.mainVptr;
  } catch {
    return undefined;
  }
}

/** Is the filesystem check pending (see `BootWindowFacts.fsMountedPtr`)? undefined when a read fails: read again. */
export async function readCheckPending(read: ReadMemory, facts: BootWindowFacts): Promise<boolean | undefined> {
  try {
    const word = async (addr: number) => u32At(await read(addr, 4));
    if ((await word(facts.fsMountedPtr)) === 0) return false;
    return (await word(facts.fsStatePtr)) === facts.fsStateCheck || (await word(facts.fsCheckedPtr)) === 0;
  } catch {
    return undefined;
  }
}

const describeTop = (t: TopView, pending?: boolean): string =>
  t.kind === 'boot-window'
    ? `boot-window(${t.result === 0 ? 'unanswered' : `result ${t.result}`})`
    : t.kind === 'other'
      ? `other(vptr 0x${t.vptr.toString(16)})`
      : t.kind === 'unknown'
        ? `unknown(${t.why})`
        : t.kind === 'main'
          ? `main(check ${pending === undefined ? 'unreadable' : pending ? 'pending' : 'not pending'})`
          : t.kind;

export interface SettleOptions {
  /** Emulated ms before the step fails (default 60 s). */
  limitMs?: number;
  /** Emulated ms between two reads (default 50). */
  pollMs?: number;
  /** Emulated ms after a NO press before the step presses again while the window is still unanswered (default 1000). */
  retapMs?: number;
  /** The one line of the observed sequence (default `console.info`). */
  log?: (line: string) => void;
}

/**
 * Settle the boot window. Each poll reads the top view and whether the bottom view is the main screen; a read that fails counts as "unknown" and is repeated.
 * The step ends only on the firmware's own state, never on a quiet time:
 *  - boot window, unanswered: tap NO (again after `retapMs` if it is still unanswered); wait until it is gone.
 *  - boot window, answered, or progress view (the check runs): wait.
 *  - main screen at the bottom and the top is neither of those (the main screen itself or an overlay such as the help bubble): the firmware has decided about the filesystem check
 *    (the same test as `MainScreenView::vfunc_5`, `readCheckPending`). Not pending: done. Pending and the window has been seen (and answered): done. Pending and no window yet: wait for the progress view and the window.
 *  - anything else (no Brain, an empty list, another bottom view, an unknown read): wait.
 * A check that ends without the window (after the progress view another ConfirmWindow is on top while the check stays pending) is named in the log and in the error.
 * Fails with the sequence seen when `limitMs` of clock time pass first. Logs the sequence once.
 */
export async function settleBootWindow(
  target: BootWindowTarget,
  facts: BootWindowFacts,
  options: SettleOptions = {},
): Promise<void> {
  const { limitMs = 60_000, pollMs = 50, retapMs = 1000, log = (line) => console.info(line) } = options;
  const { clock } = target;
  const start = await clock.now();
  const seen: string[] = [];
  let last = '';
  let lastTap: number | undefined;
  let windowSeen = false;
  let progressSeen = false;
  let failure = '';
  const note = (what: string, at: number) => {
    if (what === last) return;
    last = what;
    seen.push(`${what} @${Math.round(at - start)} ms`);
  };
  for (;;) {
    const top = await readTopView(target.read, facts);
    const now = await clock.now();
    const mainAtBottom = await readMainAtBottom(target.read, facts);
    let pending: boolean | undefined;
    if (mainAtBottom === true) pending = await readCheckPending(target.read, facts);
    note(describeTop(top, pending), now);
    if (top.kind === 'boot-window') {
      windowSeen = true;
      if (top.result === 0 && (lastTap === undefined || now - lastTap >= retapMs)) {
        await target.ui.tap('NO');
        lastTap = now;
        seen.push(`tap NO @${Math.round(now - start)} ms`);
      }
    } else if (top.kind === 'progress') {
      progressSeen = true;
    } else if (mainAtBottom === true && (pending === false || (pending === true && windowSeen))) {
      log(`boot window: ${seen.join(' -> ')}`);
      return;
    } else if (progressSeen && !windowSeen && top.kind === 'other' && top.title !== undefined) {
      const what = `the filesystem check ended without ${facts.title} (top view vptr 0x${top.vptr.toString(16)}, title "${top.title}")`;
      if (what !== failure) {
        failure = what;
        log(what);
      }
    }
    if (now - start >= limitMs)
      throw new Error(
        `${failure ? `${failure}; ` : ''}the boot window was not settled within ${limitMs} ms of emulated time (last: ${last}; seen: ${seen.join(' -> ')})`,
      );
    await clock.pause(pollMs);
  }
}
