import assert from 'node:assert/strict';
import { test } from 'vitest';
import {
  bootWindowFactsOf,
  classifyTopView,
  readCheckPending,
  readTopView,
  settleBootWindow,
  viewStackFactsOf,
  type BootWindowFacts,
  type BootWindowTarget,
} from '../src/boot-window.ts';

const facts: BootWindowFacts = {
  brainPtr: 0x1000,
  viewManagerOff: 0x40,
  listOff: 0x14,
  listNextOff: 0,
  listPrevOff: 4,
  nodeViewOff: 8,
  title: 'FILE SYSTEM OK',
  mainVptr: 0xe0,
  confirmVptr: 0xc0,
  confirmResultOff: 0x4c,
  confirmTitleOff: 0x50,
  progressVptr: 0xd0,
  fsMountedPtr: 0x1100,
  fsStatePtr: 0x1104,
  fsStateCheck: 3,
  fsCheckedPtr: 0x1108,
};
const MAIN_VPTR = facts.mainVptr;
const BRAIN = 0x2000;
const MGR = BRAIN + facts.viewManagerOff;
const SENTINEL = MGR + facts.listOff;
const NODE = 0x3000;
const VIEW = 0x4000;
const TITLE = 0x5000;
/** The vptr of an overlay (a view that is none of the known ones). */
const OVERLAY = 0x66;

/** Guest memory of big-endian words; an address that nothing wrote reads as zero. */
class Memory {
  bytes = new Map<number, number>();
  word(addr: number, value: number) {
    for (let i = 0; i < 4; i++) this.bytes.set(addr + i, (value >>> (24 - 8 * i)) & 0xff);
  }
  string(addr: number, text: string) {
    [...text, '\0'].forEach((c, i) => this.bytes.set(addr + i, c.charCodeAt(0)));
  }
  read = async (addr: number, length: number) => Uint8Array.from({ length }, (_, i) => this.bytes.get(addr + i) ?? 0);
  /** The filesystem words: mounted, state, checked. */
  fs(mounted: number, state: number, checked: number) {
    this.word(facts.fsMountedPtr, mounted);
    this.word(facts.fsStatePtr, state);
    this.word(facts.fsCheckedPtr, checked);
  }
  /** A view stack, bottom view first; a ConfirmWindow has its result and title. */
  stack(views: { vptr: number; confirm?: { result: number; title: string } }[]) {
    this.word(facts.brainPtr, BRAIN);
    const node = (i: number) => (i < 0 || i >= views.length ? SENTINEL : NODE + 0x100 * i);
    this.word(SENTINEL + facts.listNextOff, node(0));
    this.word(SENTINEL + facts.listPrevOff, node(views.length - 1));
    views.forEach(({ vptr, confirm }, i) => {
      const view = VIEW + 0x100 * i;
      this.word(node(i) + facts.listNextOff, node(i + 1));
      this.word(node(i) + facts.listPrevOff, node(i - 1));
      this.word(node(i) + facts.nodeViewOff, view);
      this.word(view, vptr);
      if (confirm) {
        this.word(view + facts.confirmResultOff, confirm.result);
        this.word(view + facts.confirmTitleOff, TITLE + 0x100 * i);
        this.string(TITLE + 0x100 * i, confirm.title);
      }
    });
  }
  /** A view stack with one view on top. */
  top(vptr: number, confirm?: { result: number; title: string }) {
    this.stack([{ vptr, confirm }]);
  }
}

test('readTopView: no Brain, an empty list, the main screen, a progress view, the boot window, another window and another title', async () => {
  const m = new Memory();
  assert.deepEqual(await readTopView(m.read, facts), { kind: 'none' });
  m.word(facts.brainPtr, BRAIN);
  m.word(SENTINEL, SENTINEL);
  m.word(SENTINEL + facts.listPrevOff, SENTINEL);
  assert.deepEqual(await readTopView(m.read, facts), { kind: 'none' });
  m.top(facts.progressVptr);
  assert.deepEqual(await readTopView(m.read, facts), { kind: 'progress' });
  m.top(facts.confirmVptr, { result: 0, title: 'FILE SYSTEM OK' });
  assert.deepEqual(await readTopView(m.read, facts), { kind: 'boot-window', result: 0 });
  m.top(facts.confirmVptr, { result: 2, title: 'FILE SYSTEM OK' });
  assert.deepEqual(await readTopView(m.read, facts), { kind: 'boot-window', result: 2 });
  m.top(facts.confirmVptr, { result: 0, title: 'FILE SYSTEM OK 2' });
  assert.deepEqual(await readTopView(m.read, facts), {
    kind: 'other',
    vptr: facts.confirmVptr,
    title: 'FILE SYSTEM OK 2',
  });
  m.top(MAIN_VPTR);
  assert.deepEqual(await readTopView(m.read, facts), { kind: 'main' });
  m.top(0x77);
  assert.deepEqual(await readTopView(m.read, facts), { kind: 'other', vptr: 0x77 });
});

test('readTopView: a throwing read and a null title pointer are unknown, not an error', async () => {
  const m = new Memory();
  m.top(MAIN_VPTR);
  const throwing: typeof m.read = async (addr, length) => {
    if (addr === VIEW) throw new Error('guest address 0x4000 is not mapped');
    return m.read(addr, length);
  };
  assert.deepEqual(await readTopView(throwing, facts), { kind: 'unknown', why: 'guest address 0x4000 is not mapped' });
  m.top(facts.confirmVptr, { result: 0, title: 'FILE SYSTEM OK' });
  m.word(VIEW + facts.confirmTitleOff, 0);
  const top = await readTopView(m.read, facts);
  assert.equal(top.kind, 'unknown');
});

test('readCheckPending: unmounted is not pending; state 3 or an unset checked word is pending; a throwing read is undefined', async () => {
  const m = new Memory();
  m.fs(0, 3, 0);
  assert.equal(await readCheckPending(m.read, facts), false);
  m.fs(1, 3, 1);
  assert.equal(await readCheckPending(m.read, facts), true);
  m.fs(1, 4, 0);
  assert.equal(await readCheckPending(m.read, facts), true);
  m.fs(1, 4, 1);
  assert.equal(await readCheckPending(m.read, facts), false);
  assert.equal(
    await readCheckPending(async () => {
      throw new Error('unmapped');
    }, facts),
    undefined,
  );
});

test('classifyTopView: a title that is a prefix or lacks the terminator is another window', () => {
  const bytes = (s: string) => Uint8Array.from([...s].map((c) => c.charCodeAt(0)));
  assert.equal(
    classifyTopView(facts, facts.confirmVptr, { result: 0, titleBytes: bytes('FILE SYSTEM OK\0') }).kind,
    'boot-window',
  );
  assert.equal(
    classifyTopView(facts, facts.confirmVptr, { result: 0, titleBytes: bytes('FILE SYSTEM OKX') }).kind,
    'other',
  );
  assert.equal(
    classifyTopView(facts, facts.confirmVptr, { result: 0, titleBytes: bytes('FILE SYSTEM O\0\0') }).kind,
    'other',
  );
});

test('bootWindowFactsOf: no [ui.boot_window] means no boot window; a missing fact throws', () => {
  assert.equal(bootWindowFactsOf({ ui: { brain_ptr: 1 } }), undefined);
  assert.equal(bootWindowFactsOf({}), undefined);
  const ui = {
    brain_ptr: 1,
    view_manager_off: 2,
    list_off: 3,
    list_next_off: 14,
    list_prev_off: 4,
    node_view_off: 5,
    boot_window: {
      title: 'T',
      main_vptr: 10,
      confirm_vptr: 6,
      confirm_result_off: 7,
      confirm_title_off: 8,
      progress_vptr: 9,
      fs_mounted_ptr: 11,
      fs_state_ptr: 12,
      fs_state_check: 3,
      fs_checked_ptr: 13,
    },
  };
  assert.deepEqual(bootWindowFactsOf({ ui }), {
    brainPtr: 1,
    viewManagerOff: 2,
    listOff: 3,
    listNextOff: 14,
    listPrevOff: 4,
    nodeViewOff: 5,
    title: 'T',
    mainVptr: 10,
    confirmVptr: 6,
    confirmResultOff: 7,
    confirmTitleOff: 8,
    progressVptr: 9,
    fsMountedPtr: 11,
    fsStatePtr: 12,
    fsStateCheck: 3,
    fsCheckedPtr: 13,
  });
  assert.throws(() => bootWindowFactsOf({ ui: { ...ui, list_off: undefined } }), /list_off/);
  assert.throws(
    () => bootWindowFactsOf({ ui: { ...ui, boot_window: { ...ui.boot_window, main_vptr: undefined } } }),
    /main_vptr/,
  );
});

/** What the scripted firmware shows at an emulated time. `fs` is [mounted, state, checked]; `fail` makes every read throw. */
interface Step {
  /** `main`: the main screen alone; `overlay`, `progress`, `window`, `other-window`: the main screen at the bottom and that view on top; `other`: a view that is not the main screen alone. */
  top: 'brain-0' | 'main' | 'overlay' | 'progress' | 'window' | 'other-window' | 'other';
  fs?: [number, number, number];
  fail?: boolean;
}
const PENDING: [number, number, number] = [1, 3, 0];
const DONE: [number, number, number] = [1, 4, 1];

/** A target on a script; NO answers the window, after which the main screen shows with the check done. */
function scripted(
  at: (ms: number) => Step,
  tapped: number[],
  afterAnswer: Step = { top: 'main', fs: DONE },
): BootWindowTarget {
  const mem = new Memory();
  let ms = 0;
  let answered = false;
  const show = (): boolean => {
    const step: Step = answered ? afterAnswer : at(ms);
    const [mounted, state, checked] = step.fs ?? DONE;
    mem.fs(mounted, state, checked);
    if (step.top === 'brain-0') mem.word(facts.brainPtr, 0);
    else if (step.top === 'main') mem.top(MAIN_VPTR);
    else if (step.top === 'other') mem.top(0x77);
    else {
      const view =
        step.top === 'overlay'
          ? { vptr: OVERLAY }
          : step.top === 'progress'
            ? { vptr: facts.progressVptr }
            : { vptr: facts.confirmVptr, confirm: { result: 0, title: step.top === 'window' ? facts.title : 'DEBUG' } };
      mem.stack([{ vptr: MAIN_VPTR }, view]);
    }
    return step.fail === true;
  };
  return {
    read: async (addr, length) => {
      if (show()) throw new Error(`guest address 0x${addr.toString(16)} is not mapped`);
      return mem.read(addr, length);
    },
    ui: {
      async tap() {
        tapped.push(ms);
        answered = true;
      },
    },
    clock: {
      now: async () => ms,
      pause: async (d) => {
        ms += d;
      },
    },
  };
}

test('settleBootWindow: no Brain, then the main screen, the progress view and the window; NO once; done when the main screen is back', async () => {
  const tapped: number[] = [];
  const lines: string[] = [];
  const target = scripted(
    (ms) =>
      ms < 200
        ? { top: 'brain-0' }
        : ms < 300
          ? { top: 'main', fs: PENDING }
          : ms < 600
            ? { top: 'progress', fs: PENDING }
            : { top: 'window', fs: DONE },
    tapped,
  );
  await settleBootWindow(target, facts, { log: (l) => lines.push(l) });
  assert.deepEqual(tapped, [600]);
  assert.equal(lines.length, 1);
  assert.match(
    lines[0]!,
    /none @0 ms -> main\(check pending\) @200 ms -> progress @300 ms -> boot-window\(unanswered\) @600 ms -> tap NO @600 ms -> main\(check not pending\) @650 ms/,
  );
});

test('settleBootWindow does not take no Brain, an empty stack or another view for settled; with the check not pending it ends at the main screen at once', async () => {
  const tapped: number[] = [];
  const target = scripted(
    (ms) => (ms < 500 ? { top: 'brain-0' } : ms < 900 ? { top: 'other' } : { top: 'main', fs: DONE }),
    tapped,
  );
  await settleBootWindow(target, facts, { log: () => {} });
  assert.deepEqual(tapped, []);
  assert.equal(await target.clock.now(), 900);
});

test('settleBootWindow waits at the main screen while the check is pending, however long it takes to start', async () => {
  const tapped: number[] = [];
  const target = scripted((ms) => (ms < 5000 ? { top: 'main', fs: PENDING } : { top: 'window', fs: DONE }), tapped);
  await settleBootWindow(target, facts, { log: () => {} });
  assert.deepEqual(tapped, [5000]);
});

test('settleBootWindow: a check whose state words do not change ends once the window was answered and the main screen is back', async () => {
  const tapped: number[] = [];
  const target = scripted(() => ({ top: 'window', fs: PENDING }), tapped, { top: 'main', fs: PENDING });
  await settleBootWindow(target, facts, { log: () => {} });
  assert.deepEqual(tapped, [0]);
});

test('settleBootWindow survives reads that fail while the list changes and ends with the error only at the limit', async () => {
  const tapped: number[] = [];
  const target = scripted(
    (ms) => (ms < 300 ? { top: 'main', fs: PENDING, fail: true } : { top: 'main', fs: DONE }),
    tapped,
  );
  await settleBootWindow(target, facts, { log: () => {} });
  assert.equal(await target.clock.now(), 300);
  const stuck = scripted(() => ({ top: 'main', fs: PENDING, fail: true }), tapped);
  await assert.rejects(
    settleBootWindow(stuck, facts, { limitMs: 500, log: () => {} }),
    /not settled within 500 ms.*last: unknown\(guest address 0x[0-9a-f]+ is not mapped\)/,
  );
});

test('settleBootWindow: the main screen at the bottom with an overlay on top and no pending check is done at once', async () => {
  const tapped: number[] = [];
  const target = scripted(() => ({ top: 'overlay', fs: DONE }), tapped);
  await settleBootWindow(target, facts, { log: () => {} });
  assert.deepEqual(tapped, []);
  assert.equal(await target.clock.now(), 0);
});

test('settleBootWindow: progress, window, NO, then an overlay on top of the main screen at the bottom is done', async () => {
  const tapped: number[] = [];
  const lines: string[] = [];
  const target = scripted((ms) => (ms < 100 ? { top: 'progress', fs: PENDING } : { top: 'window', fs: DONE }), tapped, {
    top: 'overlay',
    fs: DONE,
  });
  await settleBootWindow(target, facts, { log: (l) => lines.push(l) });
  assert.deepEqual(tapped, [100]);
  assert.match(
    lines[0]!,
    /progress @0 ms -> boot-window\(unanswered\) @100 ms -> tap NO @100 ms -> other\(vptr 0x66\)/,
  );
});

test('settleBootWindow: a check that ends in another ConfirmWindow is named in the log and the error', async () => {
  const tapped: number[] = [];
  const lines: string[] = [];
  const target = scripted(
    (ms) => (ms < 100 ? { top: 'progress', fs: PENDING } : { top: 'other-window', fs: PENDING }),
    tapped,
  );
  await assert.rejects(
    settleBootWindow(target, facts, { limitMs: 500, log: (l) => lines.push(l) }),
    /the filesystem check ended without FILE SYSTEM OK \(top view vptr 0xc0, title "DEBUG"\).*not settled within 500 ms/,
  );
  assert.deepEqual(tapped, []);
  assert.equal(lines.length, 1);
  assert.match(lines[0]!, /ended without FILE SYSTEM OK.*"DEBUG"/);
});

test('settleBootWindow presses again when the window stays unanswered, and fails with the sequence seen at the limit', async () => {
  const tapped: number[] = [];
  const target = scripted(() => ({ top: 'window', fs: DONE }), tapped);
  target.ui.tap = async () => {
    tapped.push(await target.clock.now());
  };
  await assert.rejects(
    settleBootWindow(target, facts, { limitMs: 2500, log: () => {} }),
    /not settled within 2500 ms.*last: boot-window\(unanswered\).*tap NO @0 ms.*tap NO @1000 ms/,
  );
  assert.deepEqual(tapped, [0, 1000, 2000]);
});

test('viewStackFactsOf: the view stack needs the six [ui] facts and no [ui.boot_window]; a device file without brain_ptr gives none', () => {
  const ui = { brain_ptr: 1, view_manager_off: 2, list_off: 3, list_next_off: 0, list_prev_off: 4, node_view_off: 8 };
  assert.deepEqual(viewStackFactsOf({ ui }), {
    brainPtr: 1,
    viewManagerOff: 2,
    listOff: 3,
    listNextOff: 0,
    listPrevOff: 4,
    nodeViewOff: 8,
  });
  assert.equal(viewStackFactsOf({}), undefined);
  assert.equal(viewStackFactsOf({ ui: { view_manager_off: 2 } }), undefined);
  assert.throws(() => viewStackFactsOf({ ui: { brain_ptr: 1 } }), /view_manager_off/);
});
