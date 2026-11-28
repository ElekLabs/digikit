import type { EmuRpcCall } from '@digi/dk-session';

/** The USB methods of a protocol session: they move bytes but never step the emulator. */
const USB_METHODS: ReadonlySet<string> = new Set(['midi.usb.in', 'midi.usb.out']);

export const PAUSED_PROTOCOL_MESSAGE =
  'card rig is paused: run device calls inside h.usb(rt, ...) (a free-running transport never advances a paused daemon, so it would only wait)';

/**
 * The run mode of a card rig's daemon, as the rig knows it. The rig reports each `run.mode` it sets (`set`) or a reload it cannot
 * predict (`forget`); the mode is read from the daemon (`read`) once after a `forget` and never per call.
 */
export class RigMode {
  #mode: string | undefined;
  readonly #read: () => Promise<string | undefined>;

  constructor(read: () => Promise<string | undefined>) {
    this.#read = read;
  }

  set(mode: string): void {
    this.#mode = mode;
  }

  forget(): void {
    this.#mode = undefined;
  }

  async current(): Promise<string | undefined> {
    return (this.#mode ??= await this.#read());
  }
}

/**
 * Wraps the runtime protocol's rpc of a card rig. A card rig leaves its daemon paused after a load, and the runtime's own transport polls
 * without stepping, so a call on it waits until the generic "no reply from the device". Such a call fails at once and names the fix.
 */
export function refusePausedProtocol(call: EmuRpcCall, mode: RigMode): EmuRpcCall {
  return async <T>(method: string, params?: Record<string, unknown>) => {
    if (USB_METHODS.has(method) && (await mode.current()) === 'paused') throw new Error(PAUSED_PROTOCOL_MESSAGE);
    return call<T>(method, params);
  };
}
