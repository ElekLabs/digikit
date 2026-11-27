// The query half of the device's ModuleChannel: a session's raw query endpoint as QueryResult. dk-session moves bytes; the mapping of the
// statuses to the typed errors lives here.
import { DkError, statusName, type ModuleQueryEndpoint } from '@digi/dk-session';
import type { ModuleChannel, QueryError, QueryResult } from '@digi/module-ui';

export type ModuleQueryChannel = Pick<ModuleChannel, 'callQuery'>;

const fail = (kind: QueryError['kind']): QueryResult => ({ ok: false, error: { kind } as QueryError });

/** The runtime or device status of a call that did not run the handler. */
function statusError(status: string | undefined): QueryResult {
  switch (status) {
    case 'E_BUSY':
      return fail('busy');
    case 'E_MODULE_UI_METHOD':
      return fail('unknown-method');
    case 'E_MODULE_UI_TOO_BIG':
      return fail('too-big');
    case 'E_FORMAT':
    case 'E_MSG':
      return fail('format');
    default:
      return fail('unavailable');
  }
}

/**
 * The queries of one loaded module instance over a session. The endpoint is fetched at the first call; without one the module
 * has no queries and every call is unavailable.
 */
export function deviceQueryChannel(endpoint?: () => Promise<ModuleQueryEndpoint>): ModuleQueryChannel {
  return {
    async callQuery(method, request) {
      if (!endpoint) return fail('unavailable');
      try {
        const { runtimeStatus, handlerResult, reply } = await (await endpoint()).call(method, request);
        if (runtimeStatus !== 0) return statusError(statusName(runtimeStatus));
        if (handlerResult !== 0) return { ok: false, error: { kind: 'refused', code: handlerResult } };
        return { ok: true, reply };
      } catch (e) {
        if (e instanceof DkError) {
          if (e.kind === 'timeout') return fail('timeout');
          if (e.kind === 'format') return fail('format');
          if (e.kind === 'request') return fail('too-big');
          return statusError(e.status);
        }
        return fail('unavailable');
      }
    },
  };
}
