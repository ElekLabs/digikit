// The OUT-only bulk link (USB AUDIO/MIDI): replies come from the SysEx 'R' poll in batches, with empty polls between, and chooseBulk takes the OUT-only
// candidate whose INFO carries the nonce.
import { describe, expect, test } from 'vitest';
import { BulkSession, SysExReplyLink, bulkReply, chooseBulk, parseBulkRequest, type BulkOpener } from '../src/index.ts';
import { BULK_CMD, BULK_NONCE_LEN } from '../src/gen_bulk.ts';

describe('SysExReplyLink', () => {
  test('polls until a reply is complete, splits a batch into frames, and waits only after an empty poll', async () => {
    const a = bulkReply(BULK_CMD.INFO, 1, 0, new Uint8Array(20));
    const b = bulkReply(BULK_CMD.END, 2, 0, new Uint8Array(4));
    const all = new Uint8Array([...a, ...b]);
    // empty, a frame and a half, the rest, then nothing
    const polls = [new Uint8Array(), all.subarray(0, a.length + 5), all.subarray(a.length + 5)];
    let n = 0;
    const link = new SysExReplyLink(
      async () => {},
      async () => polls[n++] ?? new Uint8Array(),
    );
    expect(await link.recv(1000)).toEqual(a);
    expect(await link.recv(1000)).toEqual(b);
    const before = n;
    expect(await link.recv(5), 'nothing pending: null after the timeout').toBeNull();
    expect(n - before, 'it kept polling').toBeGreaterThan(1);
  });
});

describe('SysExReplyLink failures and stale replies', () => {
  test('stale replies are drained before the first request, and are not taken for its answer', async () => {
    const stale = bulkReply(BULK_CMD.INFO, 1, 0, new Uint8Array(20));
    const queue = [stale, stale];
    const order: string[] = [];
    const link = new SysExReplyLink(
      async () => void order.push('out'),
      async () => {
        order.push('poll');
        return queue.shift() ?? new Uint8Array();
      },
    );
    await link.send(new Uint8Array(12));
    expect(queue).toEqual([]);
    expect(order).toEqual(['poll', 'poll', 'poll', 'out']);
    expect(await link.recv(5)).toBeNull();
  });

  test('a poll that throws ends the link with a fatal frame error', async () => {
    const link = new SysExReplyLink(
      async () => {},
      async () => {
        throw new Error('no reply');
      },
    );
    await expect(link.recv(50)).rejects.toThrow(/reply poll failed/);
    expect(link.closed).toBe(true);
  });
});

describe('chooseBulk with an OUT-only candidate', () => {
  test('INFO goes OUT, the reply comes from the poll, and the candidate with the nonce is chosen', async () => {
    let nonce: Uint8Array = new Uint8Array(BULK_NONCE_LEN);
    const queued: Uint8Array[] = [];
    const sent: number[] = [];
    const opener: BulkOpener = {
      candidates: async () => [],
      open: async () => {
        throw new Error('no pair');
      },
      outOnlyCandidates: async () => [{ id: 'dev', name: 'dev' }],
      openOutOnly: async (_id, poll) =>
        new SysExReplyLink(async (frame) => {
          const r = parseBulkRequest(frame);
          sent.push(r.cmd);
          const body = new Uint8Array(12 + BULK_NONCE_LEN);
          body.set(nonce, 12);
          queued.push(bulkReply(r.cmd, r.seq, 0, body));
        }, poll),
    };
    const sysex = {
      usbFastNonce: async (x: Uint8Array) => void (nonce = x),
      usbFastReplies: async () => queued.shift() ?? new Uint8Array(),
    };
    const choice = await chooseBulk(sysex, opener, () => {}, { timeoutMs: 200 });
    expect(choice?.id).toBe('dev');
    expect(choice?.session).toBeInstanceOf(BulkSession);
    expect(sent).toEqual([BULK_CMD.INFO]);
  });
});
