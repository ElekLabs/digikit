// INFO: the flags byte (byte 16 of the body with a 3-character port id): bit 0 safe, 1 upload open, 2 card read-only, 3 log sink, 4 fault record found.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkSession, decode8in7, encode8in7 } from '../src/index.ts';
import { CAP } from '../src/gen_proto.ts';

const concat = (...p: Uint8Array[]) => Uint8Array.from(Buffer.concat(p.map((x) => Buffer.from(x))));

function info(flags: number, card?: [number, number]): Uint8Array {
  const b = new Uint8Array(card ? 53 : 41);
  const v = new DataView(b.buffer);
  v.setUint16(0, 1);
  v.setUint16(2, 4);
  b.set(new TextEncoder().encode('dn2'), 4);
  b[16] = flags;
  v.setUint32(25, 0x000500);
  if (card) {
    v.setUint32(45, card[0]);
    v.setUint32(49, card[1]);
  }
  return b;
}

class InfoDevice {
  private queue: Uint8Array[] = [];
  private body: Uint8Array;
  constructor(body: Uint8Array) {
    this.body = body;
  }
  send(msg: Uint8Array) {
    decode8in7(msg.subarray(7, msg.length - 1));
    this.queue.push(
      concat(
        Uint8Array.of(0xf0, 0x7d, 0x64, 0x6b, 1, msg[5]! | 0x40, msg[6]!),
        encode8in7(concat(Uint8Array.of(0), this.body)),
        Uint8Array.of(0xf7),
      ),
    );
  }
  async recv(): Promise<Uint8Array | null> {
    return this.queue.shift() ?? null;
  }
}

const read = (flags: number, card?: [number, number]) =>
  new DkSession(new InfoDevice(info(flags, card)), { timeoutMs: 200, retries: 0 }).info();

test('each bit of the INFO flags byte is its own field', async () => {
  const names = ['safe_mode', 'upload_open', 'card_read_only', 'log_sink', 'fault_found'] as const;
  for (const [bit, name] of names.entries()) {
    const i = await read(1 << bit);
    for (const other of names) assert.equal(i[other], other === name, `flags ${1 << bit}: ${other}`);
  }
  const none = await read(0);
  assert.deepEqual(
    names.map((n) => none[n]),
    [false, false, false, false, false],
  );
});

test('INFO appends the card capacity and state; an older runtime reports null', async () => {
  const i = await read(0, [0x748000, 5]);
  assert.equal(i.card_sectors, 0x748000);
  assert.equal(i.card_state, 5);
  const old = await read(0);
  assert.equal(old.card_sectors, null);
  assert.equal(old.card_state, null);
});

/** A reply as the runtime writes it: the port id, flags, log range, image id (33), svc_abi (41), then fw_id (when FW_ID is set) and the card fields. */
function real(opts: { fwId?: boolean; card?: boolean }): Uint8Array {
  const caps = (1 << CAP.SVC_ABI) | (opts.fwId ? 1 << CAP.FW_ID : 0);
  const n = 45 + (opts.fwId ? 8 : 0) + (opts.card ? 8 : 0);
  const b = new Uint8Array(n);
  const v = new DataView(b.buffer);
  v.setUint16(0, 1);
  v.setUint16(2, 5);
  b.set(new TextEncoder().encode('st'), 4);
  v.setUint32(25, 0x000900);
  v.setUint32(29, caps);
  b.set([1, 2, 3, 4, 5, 6, 7, 8], 33);
  v.setUint32(41, 0xabcd1234);
  let at = 45;
  if (opts.fwId) {
    b.set([0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8], at);
    at += 8;
  }
  if (opts.card) {
    v.setUint32(at, 0x2e1a000);
    v.setUint32(at + 4, 2);
  }
  return b;
}
const readReal = (opts: { fwId?: boolean; card?: boolean }) =>
  new DkSession(new InfoDevice(real(opts)), { timeoutMs: 200, retries: 0 }).info();

test('INFO of a real reply: svc_abi, fw_id, then the card fields after it', async () => {
  const i = await readReal({ fwId: true, card: true });
  assert.equal(i.svc_abi, 0xabcd1234);
  assert.equal(i.fw_id, 'a1a2a3a4a5a6a7a8');
  assert.equal(i.card_sectors, 0x2e1a000);
  assert.equal(i.card_state, 2);
});

test('INFO with fw_id and no card fields (an older runtime) reports the card as null', async () => {
  const i = await readReal({ fwId: true });
  assert.equal(i.fw_id, 'a1a2a3a4a5a6a7a8');
  assert.equal(i.card_sectors, null);
  assert.equal(i.card_state, null);
});

test('INFO without the FW_ID bit has the card fields right after svc_abi', async () => {
  const i = await readReal({ card: true });
  assert.equal(i.fw_id, null);
  assert.equal(i.card_sectors, 0x2e1a000);
  assert.equal(i.card_state, 2);
});
