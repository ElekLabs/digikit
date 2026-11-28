import { describe, expect, it } from 'vitest';
import { PAUSED_PROTOCOL_MESSAGE, refusePausedProtocol } from './paused-protocol.ts';

const rig = (mode: string | undefined) => {
  const calls: string[] = [];
  const call = (async (method: string) => {
    calls.push(method);
    return {};
  }) as never;
  return { calls, wrapped: refusePausedProtocol(call, async () => mode) };
};

describe('refusePausedProtocol', () => {
  it('refuses USB calls on a paused daemon and names the fix', async () => {
    const { calls, wrapped } = rig('paused');
    await expect(wrapped('midi.usb.in', {})).rejects.toThrow(PAUSED_PROTOCOL_MESSAGE);
    await expect(wrapped('midi.usb.out')).rejects.toThrow('h.usb(rt');
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
});
