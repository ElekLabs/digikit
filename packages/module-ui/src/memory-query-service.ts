import {
  create,
  fromBinary,
  toBinary,
  type DescMethodUnary,
  type DescService,
  type MessageInitShape,
  type MessageShape,
} from '@bufbuild/protobuf';
import { QueryRefusal, type QueryResult } from './query-result.ts';

/** TypeScript handlers for the methods of a service, typed from its descriptor. A handler may throw `QueryRefusal` to refuse with its own code; any other throw answers `{ kind: 'refused', code: -1 }` (the device's generic handler failure), never a rejected promise. */
export type ServiceHandlers<D extends DescService> = {
  [K in keyof D['method']]: D['method'][K] extends DescMethodUnary<infer I, infer O>
    ? (request: MessageShape<I>) => MessageInitShape<O> | Promise<MessageInitShape<O>>
    : never;
};

/** The one service a module registers, with the memory channel's handlers. */
export interface MemoryQueryService {
  call(method: number, request: Uint8Array): Promise<QueryResult>;
}

export function memoryQueryService<D extends DescService>(desc: D, handlers: ServiceHandlers<D>): MemoryQueryService {
  const table = handlers as Record<string, (request: unknown) => unknown>;
  return {
    async call(number, bytes) {
      const method = desc.methods[number - 1];
      const handler = method && table[method.localName];
      if (!method || !handler) return { ok: false, error: { kind: 'unknown-method' } };
      let request: unknown;
      try {
        request = fromBinary(method.input, bytes);
      } catch {
        return { ok: false, error: { kind: 'format' } };
      }
      try {
        const reply = await handler(request);
        return { ok: true, reply: toBinary(method.output, create(method.output, reply as never)) };
      } catch (error) {
        return { ok: false, error: { kind: 'refused', code: error instanceof QueryRefusal ? error.code : -1 } };
      }
    },
  };
}
