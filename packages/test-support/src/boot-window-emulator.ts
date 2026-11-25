import {
  settleBootWindow,
  type BootWindowFacts,
  type BootWindowTarget,
  type ReadMemory,
  type SettleOptions,
} from './boot-window.ts';
import { emulatorClock } from './emulated-clock.ts';
import { panelInput } from './input-path.ts';
import type { Session } from './session.ts';

/** Guest memory through the daemon's `mem.cf.peek`, which reads one longword: `length` bytes from `addr` (any alignment). */
export function emulatorRead(session: Pick<Session, 'client'>): ReadMemory {
  return async (addr, length) => {
    const first = addr - (addr % 4);
    const out = new Uint8Array(Math.ceil((addr + length - first) / 4) * 4);
    const view = new DataView(out.buffer);
    for (let at = 0; at < out.length; at += 4) {
      const word = await session.client.memPeek(first + at);
      if (word === null) throw new Error(`guest address 0x${(first + at).toString(16)} is not mapped`);
      view.setUint32(at, word);
    }
    return out.subarray(addr - first, addr - first + length);
  };
}

/** The target of a daemon session: the panel for NO, the emulated clock (it steps a paused daemon and waits on a running one). */
export function emulatorBootTarget(session: Pick<Session, 'client' | 'status'>): BootWindowTarget {
  const clock = emulatorClock(session);
  const panel = panelInput(session, clock);
  return { read: emulatorRead(session), clock, ui: { tap: (key) => panel.tap(key, 50) } };
}

/** `settleBootWindow` on a daemon session. */
export function settleEmulatorBootWindow(
  session: Pick<Session, 'client' | 'status'>,
  facts: BootWindowFacts,
  options?: SettleOptions,
): Promise<void> {
  return settleBootWindow(emulatorBootTarget(session), facts, options);
}
