// A blocking request/response channel over one SharedArrayBuffer, for the two
// instances of the browser core: the ColdFire worker (the requester, which
// blocks like the native `ThreadedPeer`) and the DSP worker (the server).
//
// Only erasable TypeScript (it runs unchanged under Node's type stripping, see
// tools/devtools/wasm_split.mjs). Both ends are workers: `Atomics.wait` is not allowed
// on the main thread.

/** Words of the control block (Int32). */
const REQ = 0; // request counter, written by the requester
const RESP = 1; // response counter, written by the server
const KIND = 2;
const LEN = 3;
const CLOSED = 4;
const CONTROL_WORDS = 8;

export const KIND_FRAME = 0; // a DSPI2 frame: the reply has the same length
export const KIND_LINK = 1; // link port 0 bytes: no data in the reply

/** Largest payload either way (a DSPI2 frame is a few hundred bytes). */
export const MAX_PAYLOAD = 16 * 1024;

export function createLinkBuffer(): SharedArrayBuffer {
  return new SharedArrayBuffer(CONTROL_WORDS * 4 + 2 * MAX_PAYLOAD);
}

class Ends {
  ctl: Int32Array;
  tx: Uint8Array;
  rx: Uint8Array;
  constructor(buffer: SharedArrayBuffer) {
    this.ctl = new Int32Array(buffer, 0, CONTROL_WORDS);
    this.tx = new Uint8Array(buffer, CONTROL_WORDS * 4, MAX_PAYLOAD);
    this.rx = new Uint8Array(buffer, CONTROL_WORDS * 4 + MAX_PAYLOAD, MAX_PAYLOAD);
  }
}

/** The ColdFire side. */
export class LinkClient extends Ends {
  /** Milliseconds spent blocked waiting for the DSP (the ColdFire's idle time). */
  waitedMs = 0;
  /** Requests made, and how many were DSPI2 frames. */
  requests = 0;
  frames = 0;

  private request(kind: number, data: Uint8Array): void {
    if (data.length > MAX_PAYLOAD) throw new Error(`link payload of ${data.length} bytes`);
    this.tx.set(data);
    Atomics.store(this.ctl, KIND, kind);
    Atomics.store(this.ctl, LEN, data.length);
    const seq = Atomics.add(this.ctl, REQ, 1) + 1;
    Atomics.notify(this.ctl, REQ);
    const t = performance.now();
    while (Atomics.load(this.ctl, RESP) !== seq) {
      if (Atomics.load(this.ctl, CLOSED)) throw new Error('the DSP worker closed the link');
      Atomics.wait(this.ctl, RESP, seq - 1, 50);
    }
    this.waitedMs += performance.now() - t;
    this.requests++;
  }

  /** One DSPI2 frame: the DSP's reply, `data.length` bytes (a view, valid until the next call). */
  exchange(data: Uint8Array): Uint8Array {
    this.request(KIND_FRAME, data);
    this.frames++;
    return this.rx.subarray(0, data.length);
  }

  link(data: Uint8Array): void {
    this.request(KIND_LINK, data);
  }
}

/** The DSP side. */
export class LinkServer extends Ends {
  private seen = 0;
  /** Milliseconds spent inside the handler (the DSP's busy time). */
  busyMs = 0;

  /**
   * Answers requests until `close()` is called by either end. `frame` returns
   * the reply for a frame (same length); `link` consumes link bytes. `after`
   * runs after a frame's reply has been released, so the requester continues
   * while this side does the rest of the frame (the DSP's render). Returns
   * after `stopWhenIdle` ms without a request when it is set (tests).
   */
  serve(
    frame: (tx: Uint8Array) => Uint8Array,
    link: (data: Uint8Array) => void,
    after?: () => void,
    stopWhenIdle = 0,
  ): void {
    for (;;) {
      while (Atomics.load(this.ctl, REQ) === this.seen) {
        if (Atomics.load(this.ctl, CLOSED)) return;
        const r = Atomics.wait(this.ctl, REQ, this.seen, stopWhenIdle || 1000);
        if (r === 'timed-out' && stopWhenIdle) return;
      }
      const t = performance.now();
      const data = this.tx.subarray(0, Atomics.load(this.ctl, LEN));
      const isFrame = Atomics.load(this.ctl, KIND) === KIND_FRAME;
      if (isFrame) this.rx.set(frame(data));
      else link(data);
      this.seen++;
      Atomics.store(this.ctl, RESP, this.seen);
      Atomics.notify(this.ctl, RESP);
      if (isFrame && after) after();
      this.busyMs += performance.now() - t;
    }
  }

  close(): void {
    Atomics.store(this.ctl, CLOSED, 1);
    Atomics.notify(this.ctl, REQ);
    Atomics.notify(this.ctl, RESP);
  }
}
