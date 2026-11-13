import { BulkFrames, SysExReplyLink, type BulkLink } from './bulk.ts';

/**
 * A bulk link over a byte stream, such as the stdin and stdout of the `dkbulk open` bridge (packages/dkbulk). The host supplies the two ends as plain
 * functions (this file imports nothing from `node:`): `write` sends bytes, `subscribe` registers the one handler that gets every incoming chunk. Frames
 * are cut from the stream by their own length field. A corrupt stream fails the link: every later `recv` and `send` rejects with the reason.
 * (The bridge does the zero-length packets: a frame goes in whole.)
 */
export class ProcessBulkLink implements BulkLink {
  private write: (bytes: Uint8Array) => void;
  private end: () => void;
  private frames = new BulkFrames();
  private ready: Uint8Array[] = [];
  private waiter: (() => void) | null = null;
  private failure: Error | null = null;
  private outOnly: SysExReplyLink | null = null;
  /**
   * `poll` selects the OUT-only mode (`dkbulk open --out-only`, USB AUDIO/MIDI): the stream carries no replies (`subscribe` is not used), `send` writes to the
   * stream and `recv` takes the replies from `poll` (`DkSession.usbFastReplies`), see `SysExReplyLink`.
   */
  constructor(
    write: (bytes: Uint8Array) => void,
    subscribe: (onData: (chunk: Uint8Array) => void) => void,
    end: () => void = () => {},
    poll?: () => Promise<Uint8Array>,
  ) {
    this.write = write;
    this.end = end;
    if (poll) {
      this.outOnly = new SysExReplyLink(
        async (frame) => write(frame),
        poll,
        () => end(),
      );
      return;
    }
    subscribe((chunk) => {
      try {
        this.ready.push(...this.frames.feed(chunk));
      } catch (e) {
        this.failure = e instanceof Error ? e : new Error(String(e));
        this.end();
      }
      this.waiter?.();
    });
  }

  async send(frame: Uint8Array): Promise<void> {
    if (this.outOnly) return this.outOnly.send(frame);
    if (this.failure) throw this.failure;
    this.write(frame);
  }

  recv(timeoutMs: number, signal?: AbortSignal): Promise<Uint8Array | null> {
    if (this.outOnly) return this.outOnly.recv(timeoutMs, signal);
    const queued = this.ready.shift();
    if (queued) return Promise.resolve(queued);
    if (this.failure) return Promise.reject(this.failure);
    if (signal?.aborted) return Promise.resolve(null);
    return new Promise((resolve, reject) => {
      const done = (f: Uint8Array | null, err?: Error) => {
        clearTimeout(timer);
        signal?.removeEventListener('abort', onAbort);
        this.waiter = null;
        if (err) reject(err);
        else resolve(f);
      };
      const onAbort = () => done(null);
      const timer = setTimeout(() => done(null), timeoutMs);
      signal?.addEventListener('abort', onAbort, { once: true });
      this.waiter = () => {
        const f = this.ready.shift();
        if (f) done(f);
        else if (this.failure) done(null, this.failure);
      };
    });
  }

  close(): void {
    if (this.outOnly) void this.outOnly.close();
    else this.end();
  }
}
