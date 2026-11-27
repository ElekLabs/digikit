import { describe, expect, it } from 'vitest';
import { toBinary, create } from '@bufbuild/protobuf';
import {
  createMemoryModuleChannel,
  createModuleClient,
  memoryQueryService,
  QueryRefusal,
  type ModuleChannel,
} from '../src/index.ts';
import { ListReplySchema, ProjectManager } from '../../wire/src/gen/project_manager_pb.js';
import { schema } from './schema.ts';

const projects = [
  { slot: 1, name: 'A', current: true },
  { slot: 2, name: 'B', current: false },
];

function open(list = (cursor: number) => ({ items: projects.slice(cursor), next: 0 })) {
  const made = createMemoryModuleChannel(schema, {
    service: memoryQueryService(ProjectManager, {
      list: (request) => {
        if (request.cursor > 9) throw new QueryRefusal(-7);
        return list(request.cursor);
      },
    }),
  });
  return { ...made, client: createModuleClient(made.channel).service(ProjectManager) };
}

describe('client.service over the memory channel', () => {
  it('calls a typed method and returns the decoded reply', async () => {
    const result = await open().client.list({ cursor: 1 });
    expect(result).toMatchObject({ ok: true, value: { next: 0, items: [{ slot: 2, name: 'B', current: false }] } });
  });
  it('reports the handler refusal code', async () => {
    expect(await open().client.list({ cursor: 10 })).toEqual({ ok: false, error: { kind: 'refused', code: -7 } });
  });
  it('reports an unknown method number', async () => {
    const { channel } = open();
    expect(await channel.callQuery(2, new Uint8Array())).toEqual({ ok: false, error: { kind: 'unknown-method' } });
    expect(await createMemoryModuleChannel(schema).channel.callQuery(1, new Uint8Array())).toMatchObject({
      error: { kind: 'unknown-method' },
    });
  });
  it('forces busy and timeout for the next call only', async () => {
    const { client, inspect } = open();
    inspect.failNextQuery('busy');
    expect(await client.list({})).toEqual({ ok: false, error: { kind: 'busy' } });
    inspect.failNextQuery('timeout');
    expect(await client.list({})).toEqual({ ok: false, error: { kind: 'timeout' } });
    expect(await client.list({})).toMatchObject({ ok: true });
  });
  it('reports a reply that does not decode as format', async () => {
    const { channel } = open();
    const bad: ModuleChannel = { ...channel, callQuery: async () => ({ ok: true, reply: Uint8Array.of(0x0a, 0x05) }) };
    expect(await createModuleClient(bad).service(ProjectManager).list({})).toEqual({
      ok: false,
      error: { kind: 'format' },
    });
  });
  it('reports a request that does not decode as format', async () => {
    expect(await open().channel.callQuery(1, Uint8Array.of(0x08, 0x80))).toEqual({
      ok: false,
      error: { kind: 'format' },
    });
  });
  it('sends the method number and encoded request through callQuery', async () => {
    const { channel } = open();
    const reply = toBinary(ListReplySchema, create(ListReplySchema, { next: 3 }));
    const seen: [number, Uint8Array][] = [];
    const spy: ModuleChannel = {
      ...channel,
      callQuery: async (method, request) => (seen.push([method, request]), { ok: true, reply }),
    };
    expect(await createModuleClient(spy).service(ProjectManager).list({ cursor: 5 })).toMatchObject({
      value: { next: 3 },
    });
    expect(seen).toEqual([[1, Uint8Array.of(0x08, 0x05)]]);
  });
  it('returns results that survive structuredClone', async () => {
    const { client, channel } = open();
    for (const result of [await client.list({}), await client.list({ cursor: 10 })])
      expect(structuredClone(result)).toEqual(result);
    const raw = await channel.callQuery(1, new Uint8Array());
    expect(structuredClone(raw)).toEqual(raw);
  });
});
