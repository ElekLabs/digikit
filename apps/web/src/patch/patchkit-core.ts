// A thin, host-neutral wrapper over patchkit.wasm (packages/patchkit/src/wasm.rs). Used by the patch
// worker in the browser and by the node tests, so both run the same code. Everything stays in memory:
// the firmware is never sent anywhere. Written without TS-only runtime syntax so node can import it.

export interface CheckModule {
  id: string;
  ok: boolean;
  error: string | null;
}
export interface CheckResult {
  device: string;
  sha256: string;
  port: string | null;
  modules: CheckModule[];
}
export interface PatchPlan {
  base: { device: string; version: string; syx_sha256: string };
  modules: string[];
  writes: { cpu: string; addr: number; len: number; kind: string; owner: string; what: string }[];
  regions: Record<string, [number, number]>;
  record: { modules: string[] };
  output?: {
    syx_sha256: string;
    syx_bytes: number;
    container_bytes: number;
    changed_sections: [number, number, number][];
  };
}
export interface VerifyReport {
  device: string;
  container_bytes: number;
  data_messages: number;
  sections: [number, number, number][];
  record: { modules: string[] } | null;
  port: string | null;
  checks: string[];
}
export interface PatchResult {
  syx: Uint8Array;
  plan: PatchPlan;
  verify: VerifyReport;
  sha256: string;
}

/** One check of a flash (`flash::Check`): `name` is the id the interface labels. */
export interface FlashCheck {
  name: string;
  ok: boolean;
  detail: string;
}
/** `flash::Prepared::summary_json` and the image: what is checked and what would be sent. */
export interface PreparedFlash {
  /** Every check passed and there is something to send. */
  ok: boolean;
  restore: boolean;
  /** `dt2` or `dn2`. */
  device: string;
  /** The OS version of the stock image (`1.17`). */
  os: string;
  shaStock: string;
  shaImage: string;
  /** The id the runtime's INFO reports after the flash (16 hex digits); null for a restore. */
  imageId: string | null;
  /** The runtime version the patch carries (`0.5.2`); null for a restore. */
  runtime: string | null;
  paceMs: number;
  checks: FlashCheck[];
  /** End offset of each message in `image` (the first message starts at 0). */
  ends: number[];
  image: Uint8Array;
}
/** `flash::Prepared::summary_json`, as the Rust side writes it (the desktop app gets the same JSON from its Tauri command). */
export interface FlashSummary {
  ok: boolean;
  restore: boolean;
  device: string;
  os: string;
  sha_stock: string;
  sha_image: string;
  image_id: string | null;
  runtime: string | null;
  pace_ms: number;
  ends: number[];
  checks: FlashCheck[];
}

export function preparedFromSummary(j: FlashSummary, image: Uint8Array): PreparedFlash {
  return {
    ok: j.ok,
    restore: j.restore,
    device: j.device,
    os: j.os,
    shaStock: j.sha_stock,
    shaImage: j.sha_image,
    imageId: j.image_id,
    runtime: j.runtime,
    paceMs: j.pace_ms,
    checks: j.checks,
    ends: j.ends,
    image,
  };
}

/** What the sender wants next (`flash::Step`). */
export type SenderStep =
  { kind: 'send'; index: number } | { kind: 'wait'; ms: number } | { kind: 'done' } | { kind: 'aborted' };
/** `flash::Sender` (the 2 ms pacing), in the wasm module. */
export interface FlashSender {
  /** The next step at the millisecond clock `nowMs` (any monotonic clock). `send` counts the message as sent. */
  next(nowMs: number): SenderStep;
  /** An output error: the sender stops for good. */
  fail(): void;
  sent(): number;
  free(): void;
}

/** What the RPC flash machine wants next (`flash_rpc::Action`). */
export type RpcAction =
  | { kind: 'send'; bytes: Uint8Array; timeoutMs: number }
  | { kind: 'ask' }
  | { kind: 'reboot' }
  | { kind: 'done' }
  | { kind: 'failed'; code: RpcFailCode; message: string; taken: number };
export type RpcFailCode = 'refused' | 'crc' | 'timeout' | 'device' | 'protocol' | 'incomplete' | 'cancelled';
/** `flash_rpc::RpcFlash` in the wasm module: the one state machine of the RPC flash route (Start, Writes, End). */
export interface RpcMachine {
  /** The next step. Call it first and after each `reply`, `timeout`, `confirm` and `cancel`. */
  next(): RpcAction;
  /** A message from the device; false when it is not the reply awaited (keep waiting). */
  reply(msg: Uint8Array): boolean;
  /** No reply came within the action's `timeoutMs`. */
  timeout(): void;
  /** The answer to `ask`: yes sends End with commit 1, no sends End with commit 0. */
  confirm(yes: boolean): void;
  /** True when the cancel took effect (End with commit 0 follows); false when it is too late. */
  cancel(): boolean;
  /** Bytes of the image the device has acknowledged. */
  taken(): number;
  free(): void;
}

interface Exports {
  memory: WebAssembly.Memory;
  pk_alloc(n: number): number;
  pk_free(p: number, n: number): void;
  pk_apply(...a: number[]): number;
  pk_check(...a: number[]): number;
  pk_verify(...a: number[]): number;
  pk_prepare(...a: number[]): number;
  pk_sender_new(total: number): number;
  pk_sender_next(handle: number, nowMs: number): number;
  pk_sender_fail(handle: number): void;
  pk_sender_sent(handle: number): number;
  pk_sender_free(handle: number): void;
  pk_rpc_new(image: number, len: number, chunk: number): number;
  pk_rpc_next(h: number): number;
  pk_rpc_reply(h: number, msg: number, len: number): number;
  pk_rpc_timeout(h: number): void;
  pk_rpc_confirm(h: number, yes: number): void;
  pk_rpc_cancel(h: number): number;
  pk_rpc_taken(h: number): number;
  pk_rpc_free(h: number): void;
  pk_out_ptr(): number;
  pk_out_len(): number;
  pk_plan_ptr(): number;
  pk_plan_len(): number;
  pk_err_ptr(): number;
  pk_err_len(): number;
}

export class PatchKit {
  private x: Exports;
  private bundle: Uint8Array;
  private constructor(x: Exports, bundle: Uint8Array) {
    this.x = x;
    this.bundle = bundle;
  }

  static async create(wasm: BufferSource, bundleJson: string | Uint8Array): Promise<PatchKit> {
    const { instance } = await WebAssembly.instantiate(wasm, {});
    const bundle = typeof bundleJson === 'string' ? new TextEncoder().encode(bundleJson) : bundleJson;
    return new PatchKit(instance.exports as unknown as Exports, bundle);
  }

  private put(bytes: Uint8Array): [number, number] {
    const p = this.x.pk_alloc(bytes.length);
    new Uint8Array(this.x.memory.buffer, p, bytes.length).set(bytes);
    return [p, bytes.length];
  }
  private free(blocks: [number, number][]) {
    for (const [p, n] of blocks) this.x.pk_free(p, n);
  }
  private get(ptr: number, len: number): Uint8Array {
    return new Uint8Array(this.x.memory.buffer.slice(ptr, ptr + len));
  }
  private text(ptr: number, len: number): string {
    return new TextDecoder().decode(this.get(ptr, len));
  }
  private plan(): string {
    return this.text(this.x.pk_plan_ptr(), this.x.pk_plan_len());
  }
  private fail(): never {
    throw new Error(this.text(this.x.pk_err_ptr(), this.x.pk_err_len()));
  }

  /** Which bundle modules can be used on this image (a read-only pass over the manifests and the port). */
  check(firmware: Uint8Array): CheckResult {
    const m = [this.put(firmware), this.put(this.bundle)];
    try {
      if (this.x.pk_check(m[0][0], m[0][1], m[1][0], m[1][1]) !== 0) this.fail();
      return JSON.parse(this.plan()) as CheckResult;
    } finally {
      this.free(m);
    }
  }

  /**
   * `flash::prepare` (the bundle's patch set, `device` by default) or, with `restore`, `flash::restore`: the checked image and the facts the
   * Firmware page shows. A failed check is in `checks` (and `ok` is false). Throws patchkit's text for a file that is no known stock image.
   */
  prepare(stock: Uint8Array, restore: boolean, set = ''): PreparedFlash {
    const m = [this.put(stock), this.put(this.bundle), this.put(new TextEncoder().encode(set))];
    try {
      if (this.x.pk_prepare(m[0][0], m[0][1], m[1][0], m[1][1], m[2][0], m[2][1], restore ? 1 : 0) !== 0) this.fail();
      const image = this.get(this.x.pk_out_ptr(), this.x.pk_out_len());
      return preparedFromSummary(JSON.parse(this.plan()), image);
    } finally {
      this.free(m);
    }
  }

  /** A sender for `total` messages: the Rust `flash::Sender`, so the pacing has one source of truth. Call `free` when done. */
  sender(total: number): FlashSender {
    const x = this.x;
    const h = x.pk_sender_new(total);
    return {
      next(nowMs) {
        const r = x.pk_sender_next(h, nowMs);
        if (r >= 0) return { kind: 'send', index: r };
        if (r === -1) return { kind: 'done' };
        if (r === -2) return { kind: 'aborted' };
        return { kind: 'wait', ms: -3 - r };
      },
      fail: () => x.pk_sender_fail(h),
      sent: () => x.pk_sender_sent(h),
      free: () => x.pk_sender_free(h),
    };
  }

  /** The RPC flash machine for the `.syx` stream `image` (copied into the module); `chunk` is the stream bytes per Write (0: the default). Call `free` when done. */
  rpcFlash(image: Uint8Array, chunk = 0): RpcMachine {
    const x = this.x;
    const m = this.put(image);
    let h: number;
    try {
      h = x.pk_rpc_new(m[0], m[1], chunk);
    } finally {
      this.free([m]);
    }
    const read = (): RpcAction => {
      if (x.pk_rpc_next(h) !== 0) throw new Error('The flash machine is gone.');
      const j = JSON.parse(this.plan()) as {
        kind: string;
        timeout_ms?: number;
        code?: RpcFailCode;
        message?: string;
        taken?: number;
      };
      switch (j.kind) {
        case 'send':
          return { kind: 'send', bytes: this.get(x.pk_out_ptr(), x.pk_out_len()), timeoutMs: j.timeout_ms ?? 0 };
        case 'ask':
        case 'reboot':
        case 'done':
          return { kind: j.kind };
        default:
          return { kind: 'failed', code: j.code ?? 'protocol', message: j.message ?? '', taken: j.taken ?? 0 };
      }
    };
    return {
      next: read,
      reply: (msg) => {
        const b = this.put(msg);
        try {
          return x.pk_rpc_reply(h, b[0], b[1]) === 1;
        } finally {
          this.free([b]);
        }
      },
      timeout: () => x.pk_rpc_timeout(h),
      confirm: (yes) => x.pk_rpc_confirm(h, yes ? 1 : 0),
      cancel: () => x.pk_rpc_cancel(h) === 1,
      taken: () => x.pk_rpc_taken(h),
      free: () => x.pk_rpc_free(h),
    };
  }

  /** Apply the modules (patchkit orders them) and verify the result against the stock image. Throws patchkit's error text. */
  apply(firmware: Uint8Array, ids: string[]): PatchResult {
    const m = [this.put(firmware), this.put(this.bundle), this.put(new TextEncoder().encode(ids.join(',')))];
    try {
      if (this.x.pk_apply(m[0][0], m[0][1], m[1][0], m[1][1], m[2][0], m[2][1]) !== 0) this.fail();
      const syx = this.get(this.x.pk_out_ptr(), this.x.pk_out_len());
      const plan = JSON.parse(this.plan()) as PatchPlan;
      const v = [this.put(syx), this.put(firmware)];
      try {
        if (this.x.pk_verify(v[0][0], v[0][1], m[1][0], m[1][1], v[1][0], v[1][1]) !== 0) this.fail();
        const verify = JSON.parse(this.plan()) as VerifyReport;
        return { syx, plan, verify, sha256: plan.output?.syx_sha256 ?? '' };
      } finally {
        this.free(v);
      }
    } finally {
      this.free(m);
    }
  }
}

/** The file name of a patched image: `Digitakt_II_OS1.17+digikit-master-gain.syx`. */
export function patchedFileName(deviceName: string, version: string, ids: string[]): string {
  const base = `${deviceName.replace(/\s+/g, '_')}_OS${version}`;
  return `${base}+${ids.map((id) => id.replace(/\//g, '-')).join('+')}.syx`;
}
