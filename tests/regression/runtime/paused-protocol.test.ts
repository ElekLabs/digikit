import { describe, expect, it } from 'vitest';
import { refusePausedProtocol, RigMode } from './paused-protocol.ts';

const rig = (initial: string | undefined) => {
  const calls: string[] = [];
  let reads = 0;
  const call = (async (method: string) => {
    calls.push(method);
    return {};
  }) as never;
  const mode = new RigMode(async () => (reads++, initial));
  return { calls, mode, reads: () => reads, wrapped: refusePausedProtocol(call, mode) };
};

describe('refusePausedProtocol', () => {
  it('refuses USB calls on a paused daemon without forwarding them and points at h.usb', async () => {
    const { calls, wrapped } = rig('paused');
    await expect(wrapped('midi.usb.in', {})).rejects.toThrow('h.usb(');
    await expect(wrapped('midi.usb.out')).rejects.toThrow();
    expect(calls).toEqual([]);
  });

  it('passes USB calls on a free daemon and other calls while paused', async () => {
    const free = rig('free');
    await free.wrapped('midi.usb.in', {});
    expect(free.calls).toEqual(['midi.usb.in']);
    const paused = rig('paused');
    await paused.wrapped('status');
    expect(paused.calls).toEqual(['status']);
  });

  it('reads the daemon once, then follows what the rig sets', async () => {
    const { mode, wrapped, calls, reads } = rig('paused');
    await expect(wrapped('midi.usb.in')).rejects.toThrow();
    await expect(wrapped('midi.usb.in')).rejects.toThrow();
    expect(reads()).toBe(1);
    mode.set('free');
    await wrapped('midi.usb.in');
    expect(calls).toEqual(['midi.usb.in']);
    mode.set('paused');
    await expect(wrapped('midi.usb.in')).rejects.toThrow();
    mode.forget();
    await expect(wrapped('midi.usb.in')).rejects.toThrow();
    expect(reads()).toBe(2);
  });
});
