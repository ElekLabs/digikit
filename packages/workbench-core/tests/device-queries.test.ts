import { describe, expect, it } from 'vitest';
import { DkError, wireModuleQueries, type DkSession, type ModuleQueryEndpoint, type RawQueryReply } from '@digi/dk-session';
import { deviceQueryChannel } from '../src/module-ui/device-queries.ts';

const reply = (runtimeStatus: number, handlerResult = 0, bytes: number[] = []): RawQueryReply => ({
  runtimeStatus,
  handlerResult,
  reply: Uint8Array.from(bytes),
});
const channel = (outcome: RawQueryReply | Error) =>
  deviceQueryChannel(async () => ({
    call: async () => {
      if (outcome instanceof Error) throw outcome;
      return outcome;
    },
  }));
const call = (outcome: RawQueryReply | Error) => channel(outcome).callQuery(1, new Uint8Array());
const device = (status: string) => new DkError({ error: status, kind: 'device', status });

describe('deviceQueryChannel', () => {
  it('maps a handler that ran to a reply or to its own refusal code', async () => {
    expect(await call(reply(0, 0, [1, 2]))).toEqual({ ok: true, reply: Uint8Array.of(1, 2) });
    expect(await call(reply(0, -7))).toEqual({ ok: false, error: { kind: 'refused', code: -7 } });
  });

  it.each([
    ['busy', device('E_BUSY')],
    ['unknown-method', reply(-90)],
    ['too-big', reply(-91)],
    ['unavailable', reply(-1)],
    ['unavailable', device('E_NOT_FOUND')],
    ['format', device('E_FORMAT')],
    ['format', new DkError({ error: 'short', kind: 'format' })],
    ['timeout', new DkError({ error: 'late', kind: 'timeout' })],
    ['too-big', new DkError({ error: 'big', kind: 'request' })],
    ['unavailable', new Error('The module page has closed.')],
  ] as const)('maps %s', async (kind, outcome) => {
    expect(await call(outcome)).toEqual({ ok: false, error: { kind } });
  });

  it('answers unavailable for a module without queries', async () => {
    expect(await deviceQueryChannel().callQuery(1, new Uint8Array())).toEqual({
      ok: false,
      error: { kind: 'unavailable' },
    });
  });

  it('hands the method and the request to the endpoint', async () => {
    const seen: [number, Uint8Array][] = [];
    const endpoint: ModuleQueryEndpoint = { call: async (m, r) => (seen.push([m, r]), reply(0)) };
    await deviceQueryChannel(async () => endpoint).callQuery(3, Uint8Array.of(4));
    expect(seen).toEqual([[3, Uint8Array.of(4)]]);
  });

  it('answers unavailable when the instance goes away while the call is polled', async () => {
    let live = true;
    let now = 0;
    const session: Pick<DkSession, 'clock' | 'request'> = {
      clock: { now: async () => now, pause: async (ms) => void (now += ms) },
      async request(cmd, body = new Uint8Array()) {
        // The call is accepted; the first poll says pending, and the instance is gone before the next one.
        if (body[0] === 4) live = false;
        return { cmd, seq: 0, status: 0, body: body[0] === 4 ? Uint8Array.of(1) : new Uint8Array() };
      },
    };
    const result = await deviceQueryChannel(async () => wireModuleQueries(session, 1, 1, () => live)).callQuery(
      1,
      new Uint8Array(),
    );
    expect(result).toEqual({ ok: false, error: { kind: 'unavailable' } });
  });
});
