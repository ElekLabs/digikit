// The call verb of a loaded module's Workbench queries: one call envelope, polled until the runtime answers. It moves bytes; the
// status mapping to a QueryResult lives in workbench-core.
import { QUERY_REPLY_MAX, QUERY_REQUEST_MAX } from './gen_proto.ts';
import { be32, DkError, type DkSession } from './dkupload-core.ts';
import { moduleUiRequester } from './module-ui-request.ts';
import { poll } from './waits.ts';

/** The raw answer of one query: the runtime's status (0 when the handler ran), the handler's own result and its reply bytes. */
export interface RawQueryReply {
  runtimeStatus: number;
  handlerResult: number;
  reply: Uint8Array;
}

/** The transport of one loaded module's queries. */
export interface ModuleQueryEndpoint {
  call(method: number, request: Uint8Array): Promise<RawQueryReply>;
}

const CALL = 5;
const POLL = 4;
/** The device answers a second call with busy until the first was polled or 2 s passed; one call waits a little longer than that. */
const CALL_WITHIN_MS = 3000;

export function wireModuleQueries(
  session: Pick<DkSession, 'request' | 'clock'>,
  slot: number,
  instance: number,
  current: () => boolean,
): ModuleQueryEndpoint {
  const send = moduleUiRequester(session, slot, instance, current);
  return {
    async call(method, request) {
      if (!Number.isInteger(method) || method < 1 || method > 255) throw new Error('Invalid module query method.');
      if (request.length > QUERY_REQUEST_MAX)
        throw new DkError({
          error: `The query request is ${request.length} bytes; at most ${QUERY_REQUEST_MAX} fit.`,
          kind: 'request',
        });
      const ticket = be32(crypto.getRandomValues(new Uint32Array(1))[0]! || 1);
      await send(CALL, Uint8Array.of(...ticket, method, ...request), 'Module query');
      let bytes: Uint8Array;
      try {
        bytes = await poll(
          session.clock,
          () => send(POLL, ticket, 'Module query'),
          (b) => b.length !== 1 || b[0] !== 1,
          { withinMs: CALL_WITHIN_MS, intervalMs: 30, message: 'Waiting for a module query' },
        );
      } catch (e) {
        if (e instanceof Error && !(e instanceof DkError) && /not seen within/.test(e.message))
          throw new DkError({ error: e.message, kind: 'timeout' });
        throw e;
      }
      if (bytes.length < 9 || bytes[0] !== 0 || bytes.length - 9 > QUERY_REPLY_MAX)
        throw new DkError({ error: 'Invalid module query reply.', kind: 'format' });
      const view = new DataView(bytes.buffer, bytes.byteOffset);
      return { runtimeStatus: view.getInt32(1), handlerResult: view.getInt32(5), reply: bytes.slice(9) };
    },
  };
}
