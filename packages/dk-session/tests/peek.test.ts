// PEEK in the pure TypeScript client (`DkSession.peek`, dkupload-core.ts) against a fake device whose RAM is a byte array: a read longer than one
// request is made of several, and a range the device file does not list is the device's E_PEEK.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkSession, decode8in7, encode8in7 } from '@digi/dk-session';
import { PEEK_MAX, SERVICE_CMD } from '@digi/dk-session/gen_proto';

const PREFIX = [0xf0, 0x7d, 0x64, 0x6b, 1];
const concat = (...p: Uint8Array[]) => Uint8Array.from(Buffer.concat(p.map((x) => Buffer.from(x))));
const reply = (cmd: number, seq: number, status: number, body: number[] = []) =>
  concat(
    Uint8Array.from(PREFIX),
    Uint8Array.of(cmd | 0x40, seq),
    encode8in7(Uint8Array.of(status & 255, ...body)),
    Uint8Array.of(0xf7),
  );

const BASE = 0x40100000;
const RAM = Uint8Array.from({ length: 1500 }, (_, i) => (i * 7) & 255);

class FakeDebug {
  requests: [number, number][] = [];
  queue: Uint8Array[] = [];
  send(msg: Uint8Array) {
    const body = decode8in7(msg.subarray(7, msg.length - 1));
    const view = new DataView(body.buffer, body.byteOffset);
    const [addr, len] = [view.getUint32(0), view.getUint16(4)];
    this.requests.push([addr, len]);
    const inside = addr >= BASE && addr + len <= BASE + RAM.length && len >= 1 && len <= PEEK_MAX;
    const head = [addr >>> 24, (addr >> 16) & 255, (addr >> 8) & 255, addr & 255, len >> 8, len & 255];
    this.queue.push(
      inside
        ? reply(msg[5]!, msg[6]!, 0, [...head, ...RAM.subarray(addr - BASE, addr - BASE + len)])
        : reply(msg[5]!, msg[6]!, -63),
    );
  }
  async recv() {
    return this.queue.shift() ?? null;
  }
}

test('a long read is several PEEK requests of at most PEEK_MAX bytes, joined', async () => {
  const dev = new FakeDebug();
  const s = new DkSession(dev, { timeoutMs: 200 });
  const bytes = await s.peek(BASE + 4, 1300);
  assert.deepEqual([...bytes], [...RAM.subarray(4, 1304)]);
  assert.deepEqual(dev.requests, [
    [BASE + 4, 512],
    [BASE + 516, 512],
    [BASE + 1028, 276],
  ]);
  assert.equal(SERVICE_CMD.PEEK, 0x21);
});

test('a range the device does not allow is the device error', async () => {
  const s = new DkSession(new FakeDebug(), { timeoutMs: 200 });
  await assert.rejects(s.peek(BASE - 4, 8), /E_PEEK|peek/i);
});
