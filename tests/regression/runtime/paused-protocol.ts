import type { EmuRpcCall } from '@digi/dk-session';

/** The USB methods of a protocol session: they move bytes but never step the emulator. */
const USB_METHODS: ReadonlySet<string> = new Set(['midi.usb.in', 'midi.usb.out']);

export const PAUSED_PROTOCOL_MESSAGE =
  'card rig is paused: run device calls inside h.usb(rt, ...) (a free-running transport never advances a paused daemon, so it would only wait)';

/**
 * Wraps the runtime protocol's rpc of a card rig. A card rig leaves its daemon paused after a load, and the runtime's own transport polls
 * without stepping, so a call on it waits until the generic "no reply from the device". Such a call fails at once and names the fix;
 * the mode is read from the daemon, so a test that freed the daemon itself keeps working.
 */
export function refusePausedProtocol(call: EmuRpcCall, mode: () => Promise<string | undefined>): EmuRpcCall {
  return async <T>(method: string, params?: Record<string, unknown>) => {
    if (USB_METHODS.has(method) && (await mode()) === 'paused') throw new Error(PAUSED_PROTOCOL_MESSAGE);
    return call<T>(method, params);
  };
}
