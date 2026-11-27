import {
  fromBinary,
  toBinary,
  create,
  type DescMessage,
  type DescMethodUnary,
  type DescService,
  type MessageInitShape,
  type MessageShape,
} from '@bufbuild/protobuf';
import type { ModuleChannel } from './module-channel.ts';
import type { QueryOutcome } from './query-result.ts';

/** One async function per unary method of a protobuf-es service, named as protobuf-es names methods (lowerCamel). */
export type ServiceClient<D extends DescService> = {
  [K in keyof D['method']]: D['method'][K] extends DescMethodUnary<infer I, infer O>
    ? (request: MessageInitShape<I>) => Promise<QueryOutcome<MessageShape<O>>>
    : never;
};

/** Builds the client of `desc` over `channel.callQuery`. The method number is the method's 1-based position in the service. */
export function createServiceClient<D extends DescService>(channel: ModuleChannel, desc: D): ServiceClient<D> {
  const client: Record<string, unknown> = {};
  desc.methods.forEach((method, index) => {
    if (method.methodKind !== 'unary') throw new Error(`${method.name}: only unary methods can be queried.`);
    const input: DescMessage = method.input;
    const output: DescMessage = method.output;
    client[method.localName] = async (request: unknown): Promise<QueryOutcome<unknown>> => {
      const result = await channel.callQuery(index + 1, toBinary(input, create(input, request as never)));
      if (!result.ok) return result;
      try {
        return { ok: true, value: fromBinary(output, result.reply) };
      } catch {
        return { ok: false, error: { kind: 'format' } };
      }
    };
  });
  return client as ServiceClient<D>;
}
