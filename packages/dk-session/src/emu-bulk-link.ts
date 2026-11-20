// The USB bulk link to the emulator: `BulkLink` over the control daemon's `usb.bulk.in` / `usb.bulk.out` (docs/CONTROL.md), shaped like emu-usb-transport.ts.
// The emulator does not model the USB controller: `usb.bulk.in` fills the dTDs the guest's core/usb-bulk adapter has queued on EP2 OUT and runs their completion
// callbacks at interrupt level, `usb.bulk.out` returns what the adapter queued on EP2 IN. Used by the desktop workbench's Emulator target (so it negotiates
// Fast USB like a real device) and by the device tests. Written without TS-only runtime syntax so node can import it.
import { BulkFrames, SysExReplyLink, sendWithZlp, type BulkLink } from './bulk.ts';
import type { BulkCandidate, BulkOpener } from './bulk-select.ts';
import type { EmuRpcCall } from './emu-usb-transport.ts';

/** The pointer operand of the firmware's USB mode switch (`cf.usb.data.port_table`, devices/fw/usb.toml): the same address on DT2 1.17 and DN2 1.12. */
const PORT_TABLE_POINTER = 0x40006c38;
const PORT_ENTRY_BYTES = 0x28;
/** USB MIDI is port entry 2: +0 is its connect callback. */
const MIDI_PORT = 2;
/** The stock connect callback of that entry (`FUN_400044b8`): the adapter has not taken the entry over. */
const STOCK_CONNECT = 0x400044b8;

/** USB AUDIO/MIDI is port entry 5; once core/usb-bulk-audio has attached interface 5, its config length (+0x10) is 0x158 (stock: 0x148). */
const AUDIO_PORT = 5;
const PORT_CONFIG_LENGTH = 0x10;
const ATTACHED_CONFIG_LENGTH = 0x158;

/** The boot (status.epoch) of the guest whose bus reset was delivered, per control connection. */
const booted = new WeakMap<object, number>();

const hex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
const bytesOf = (h: string) => Uint8Array.from(h.match(/../g) ?? [], (x) => parseInt(x, 16));

export interface EmuBulkOptions {
  /**
   * The adapter's connect callback (`usb_bulk_connect`): the bus reset after the enumeration, which the emulator does not model, is delivered through it
   * before the first frame. Default: read from the guest's port table (`EmuBulkLink.connectCallback`).
   */
  connect?: number;
  /** Instructions between deliveries of a 16 KiB transfer (the daemon's default is one high-speed transfer). */
  period?: number;
  /** For a paused daemon, where time passes only when stepped: an empty poll steps this many instructions. Unset: a free-running daemon, polled every `pollMs`. */
  stepInstructions?: number;
  pollMs?: number;
  /** Send the zero-length packet after a frame of a multiple of 512 bytes (default). false: the transfer stays open (tests of the rule). */
  zlp?: boolean;
}

export class EmuBulkLink implements BulkLink {
  private readonly call: EmuRpcCall;
  private readonly o: EmuBulkOptions;
  private frames = new BulkFrames();
  private ready: Uint8Array[] = [];
  private connected = false;
  private connect: number | undefined;
  /** Bytes sent and received, for benches. */
  sentBytes = 0;
  receivedBytes = 0;

  constructor(call: EmuRpcCall, o: EmuBulkOptions = {}) {
    this.call = call;
    this.o = o;
    this.connect = o.connect;
  }

  /**
   * The connect callback the guest has installed in the port table, or undefined when the adapter has not taken entry 2 over (no module that enables the bulk
   * interface is loaded yet, or none is selected).
   */
  static async connectCallback(call: EmuRpcCall): Promise<number | undefined> {
    const peek = async (addr: number) => {
      const r = await call<{ value: number | string | null }>('mem.cf.peek', { addr });
      return r.value === null || r.value === undefined ? null : Number(r.value);
    };
    const table = await peek(PORT_TABLE_POINTER);
    if (!table) return undefined;
    const connect = await peek(table + MIDI_PORT * PORT_ENTRY_BYTES);
    return connect && connect !== STOCK_CONNECT ? connect : undefined;
  }

  /** The devices the app can open: one `emulator` candidate while the guest has a bulk interface. */
  static opener(call: EmuRpcCall, o: EmuBulkOptions = {}): BulkOpener {
    return {
      async candidates(): Promise<BulkCandidate[]> {
        return (await EmuBulkLink.connectCallback(call)) === undefined ? [] : [{ id: 'emulator', name: 'emulator' }];
      },
      async open() {
        return new EmuBulkLink(call, o);
      },
    };
  }

  private async out(data: Uint8Array): Promise<void> {
    this.sentBytes += data.length;
    let connect: number | undefined;
    if (!this.connected) {
      connect = this.connect ?? (await EmuBulkLink.connectCallback(this.call));
      this.connect = connect;
      if (connect === undefined) throw new Error('the guest has no bulk interface (core/usb-bulk is not enabled)');
      this.connected = true;
      // the bus reset follows the enumeration once per boot of the guest, not once per link a host opens
      const boot = Number((await this.call<{ epoch?: number }>('status')).epoch ?? 0);
      if (booted.get(this.call) === boot) connect = undefined;
      else booted.set(this.call, boot);
    }
    const params: Record<string, unknown> = { zlp: data.length === 0 };
    if (data.length) params.bytes = hex(data);
    if (this.o.period) params.period = this.o.period;
    if (connect !== undefined) params.connect = connect;
    await this.call('usb.bulk.in', params);
  }

  async send(frame: Uint8Array): Promise<void> {
    if (this.o.zlp === false) return this.out(frame);
    await sendWithZlp((d) => this.out(d), frame);
  }

  async recv(timeoutMs: number, signal?: AbortSignal): Promise<Uint8Array | null> {
    const end = Date.now() + timeoutMs;
    for (;;) {
      const next = this.ready.shift();
      if (next) return next;
      if (signal?.aborted) return null;
      const { hex: got, status } = await this.call<{ hex?: string; status?: { usb_sys_error?: string | null } }>(
        'usb.bulk.out',
      );
      if (typeof status?.usb_sys_error === 'string' && status.usb_sys_error !== '') {
        throw new Error(`emulated USB controller system error: ${status.usb_sys_error}`);
      }
      if (got) {
        const b = bytesOf(got);
        this.receivedBytes += b.length;
        this.ready.push(...this.frames.feed(b));
        continue;
      }
      if (Date.now() >= end || signal?.aborted) return null;
      if (this.o.stepInstructions && this.o.stepInstructions > 0)
        await this.call('step', { instructions: this.o.stepInstructions });
      else await new Promise((r) => setTimeout(r, this.o.pollMs ?? 10));
    }
  }

  close(): void {
    this.ready = [];
  }
}

/**
 * The emulator's OUT-only link (USB AUDIO/MIDI, port entry 5 with interface 5 attached): frames go OUT through `usb.bulk.in` like `EmuBulkLink` (EP2 OUT),
 * the replies come from `poll` (`DkSession.usbFastReplies`, the 'R' poll), because the guest has no bulk IN there. `o.connect` is the connect callback of
 * port entry 5 (`EmuOutOnlyLink.connectCallback`); `opener` supplies it.
 */
export class EmuOutOnlyLink extends SysExReplyLink {
  readonly bulk: EmuBulkLink;
  constructor(call: EmuRpcCall, poll: () => Promise<Uint8Array>, o: EmuBulkOptions = {}) {
    const bulk = new EmuBulkLink(call, o);
    super((frame) => bulk.send(frame), poll);
    this.bulk = bulk;
  }

  /** The connect callback of port entry 5 while the guest has attached the OUT-only interface, else undefined. */
  static async connectCallback(call: EmuRpcCall): Promise<number | undefined> {
    const peek = async (addr: number) => {
      const r = await call<{ value: number | string | null }>('mem.cf.peek', { addr });
      return r.value === null || r.value === undefined ? null : Number(r.value);
    };
    const table = await peek(PORT_TABLE_POINTER);
    if (!table) return undefined;
    const entry = table + AUDIO_PORT * PORT_ENTRY_BYTES;
    if ((await peek(entry + PORT_CONFIG_LENGTH)) !== ATTACHED_CONFIG_LENGTH) return undefined;
    return (await peek(entry)) || undefined;
  }

  /** An opener with an `emulator` OUT-only candidate while the guest has attached the interface, and no pair candidate. */
  static opener(call: EmuRpcCall, o: EmuBulkOptions = {}): BulkOpener {
    return {
      async candidates(): Promise<BulkCandidate[]> {
        return [];
      },
      async open(): Promise<BulkLink> {
        throw new Error('the emulator has no bulk pair in USB AUDIO/MIDI');
      },
      async outOnlyCandidates(): Promise<BulkCandidate[]> {
        return (await EmuOutOnlyLink.connectCallback(call)) === undefined ? [] : [{ id: 'emulator', name: 'emulator' }];
      },
      async openOutOnly(_id, poll) {
        const connect = o.connect ?? (await EmuOutOnlyLink.connectCallback(call));
        return new EmuOutOnlyLink(call, poll, { ...o, connect });
      },
    };
  }
}
