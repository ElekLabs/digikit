import { expect, test } from 'vitest';
import { be32, wireModuleQueries, type DkSession } from '../src/index.ts';
import { CMD, QUERY_REQUEST_MAX } from '../src/gen_proto.ts';

const answer = (runtime: number, handler: number, ...reply: number[]) =>
  Uint8Array.of(0, ...be32(runtime), ...be32(handler), ...reply);

function scripted(replies: (Uint8Array | number)[]) {
  const requests: Uint8Array[] = [];
  let now = 0;
  const session: Pick<DkSession, 'clock' | 'request'> = {
    clock: {
      now: async () => now,
      pause: async (ms) => {
        now += ms;
      },
    },
    async request(cmd, body = new Uint8Array()) {
      expect(cmd).toBe(CMD.MODULE_UI);
      requests.push(body);
      const reply = replies.shift();
      if (reply === undefined) throw new Error('Unexpected request.');
      return typeof reply === 'number'
        ? { cmd, seq: 0, status: reply, body: new Uint8Array() }
        : { cmd, seq: 0, status: 0, body: reply };
    },
  };
  return { session, requests };
}

test('a call sends the envelope, polls with its request id until done and returns the raw statuses and reply', async () => {
  const { session, requests } = scripted([new Uint8Array(), Uint8Array.of(1), answer(0, -5, 7, 8)]);
  const result = await wireModuleQueries(session, 3, 0x1234, () => true).call(2, Uint8Array.of(9, 10));
  expect(result).toEqual({ runtimeStatus: 0, handlerResult: -5, reply: Uint8Array.of(7, 8) });
  const call = requests[0]!;
  expect(Array.from(call.slice(0, 6))).toEqual([5, 3, 0, 0, 0x12, 0x34]);
  expect(Array.from(call.slice(10))).toEqual([2, 9, 10]);
  expect(Array.from(requests[1]!.slice(0, 6))).toEqual([4, 3, 0, 0, 0x12, 0x34]);
  expect(requests[1]!.slice(6)).toEqual(call.slice(6, 10));
});

test('a request over the limit is refused before anything is sent', async () => {
  const { session, requests } = scripted([]);
  const endpoint = wireModuleQueries(session, 0, 1, () => true);
  await expect(endpoint.call(1, new Uint8Array(QUERY_REQUEST_MAX + 1))).rejects.toThrow(/at most/);
  expect(requests).toEqual([]);
});

test('a busy refusal of the call is a device error with its status name', async () => {
  const { session } = scripted([-12]);
  await expect(wireModuleQueries(session, 0, 1, () => true).call(1, new Uint8Array())).rejects.toMatchObject({
    status: 'E_BUSY',
  });
});

test('a call that never finishes times out in clock time', async () => {
  const { session } = scripted([new Uint8Array(), ...Array.from({ length: 300 }, () => Uint8Array.of(1))]);
  await expect(wireModuleQueries(session, 0, 1, () => true).call(1, new Uint8Array())).rejects.toMatchObject({
    kind: 'timeout',
  });
});

test('a reply that is too short is a format error', async () => {
  const { session } = scripted([new Uint8Array(), Uint8Array.of(0, 0, 0)]);
  await expect(wireModuleQueries(session, 0, 1, () => true).call(1, new Uint8Array())).rejects.toMatchObject({
    kind: 'format',
  });
});
