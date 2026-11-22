import { withinRuntimeDeadline, type Session } from '@digi/test-support';

/** The calls of a test on a running session: each ends within 30 s of wall time or throws. */
export const bounded = <T>(operation: () => Promise<T>) => withinRuntimeDeadline(Date.now() + 30_000, operation);

export const rpc = <T>(session: Session, method: string, params?: Record<string, unknown>) =>
  bounded(() => session.client.call<T>(method, params));

/** One CF word of the guest at `address`; throws when it is unmapped. */
export async function word(session: Session, address: number): Promise<number> {
  const value = await bounded(() => session.client.memPeek(address));
  if (value === null) throw new Error(`unmapped CF word at ${address.toString(16)}`);
  return value >>> 0;
}
