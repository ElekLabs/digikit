/** Why a query did not return a reply. `refused` carries the module handler's own result code. */
export type QueryError =
  | { kind: 'busy' }
  | { kind: 'refused'; code: number }
  | { kind: 'unavailable' }
  | { kind: 'too-big' }
  | { kind: 'format' }
  | { kind: 'unknown-method' }
  | { kind: 'timeout' };

/** The outcome of one query as the channel reports it: reply bytes or the reason there are none. Structured-clone data only. */
export type QueryResult = { ok: true; reply: Uint8Array } | { ok: false; error: QueryError };

/** The outcome of a typed query call. */
export type QueryOutcome<T> = { ok: true; value: T } | { ok: false; error: QueryError };

/** A module handler in the memory channel throws this to refuse a request with its own code. */
export class QueryRefusal extends Error {
  readonly code: number;
  constructor(code: number) {
    super(`Query refused with code ${code}.`);
    this.name = 'QueryRefusal';
    this.code = code;
  }
}
