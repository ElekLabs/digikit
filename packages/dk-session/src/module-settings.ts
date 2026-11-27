// A public settings endpoint belongs to one loaded module instance on one session.
import { CMD } from './gen_proto.ts';
import { be32, deviceError, DkError, statusName, u32, type DkSession } from './dkupload-core.ts';
import { poll } from './waits.ts';

/** A stored value as the device holds it: raw bytes (absent while unset) and the setting's revision. */
export interface RawSettingValue {
  value?: Uint8Array;
  revision: number;
}

/** The transport of one loaded module's public settings. It moves bytes; `@digi/module-ui` decodes the descriptor and the values. */
export interface ModuleSettingsEndpoint {
  /** The binary descriptor the module registered. */
  describe(): Promise<Uint8Array>;
  get(field: number): Promise<RawSettingValue>;
  put(field: number, value: Uint8Array, revision: number): Promise<RawSettingValue>;
  reset(field: number, revision: number): Promise<RawSettingValue>;
}

export function wireModuleSettings(
  session: Pick<DkSession, 'request' | 'clock'>,
  slot: number,
  instance: number,
  current: () => boolean,
): ModuleSettingsEndpoint {
  const check = () => {
    if (!current()) throw new Error('The module page has closed.');
  };
  async function request(verb: number, suffix: Uint8Array = new Uint8Array()) {
    check();
    const body = Uint8Array.of(verb, slot, ...be32(instance), ...suffix);
    const reply = await session.request(CMD.MODULE_UI, body);
    check();
    if (reply.status) throw deviceError('Module settings', reply.status);
    return reply.body;
  }
  let turn = Promise.resolve();
  async function exchange(verb: number, field: number, revision: number, payload: Uint8Array) {
    const ticket = be32(crypto.getRandomValues(new Uint32Array(1))[0]! || 1);
    await request(verb, Uint8Array.of(...ticket, field, ...be32(revision), ...payload));
    const bytes = await poll(
      session.clock,
      () => request(4, ticket),
      (b) => b.length !== 1 || b[0] !== 1,
      {
        withinMs: 5000,
        intervalMs: 30,
        message: 'Waiting for module settings',
      },
    );
    if (bytes.length < 10 || bytes[0] !== 0) throw new Error('Invalid module settings reply.');
    const result = new DataView(bytes.buffer, bytes.byteOffset).getInt32(1);
    if (result !== 0) {
      if (statusName(result) === 'E_MODULE_UI_CONFLICT')
        throw new DkError({
          error: 'The setting changed. Read it again before saving.',
          kind: 'device',
          status: 'E_MODULE_UI_CONFLICT',
        });
      throw Object.assign(deviceError('Module settings', result), { revision: u32(bytes, 5) });
    }
    if (bytes[9]! > 1 || (!bytes[9] && bytes.length !== 10)) throw new Error('Invalid module setting value.');
    return { revision: u32(bytes, 5), value: bytes[9] ? bytes.slice(10) : undefined };
  }
  function submit(verb: number, field: number, revision = 0, bytes: Uint8Array = new Uint8Array()) {
    if (
      !Number.isInteger(field) ||
      field < 0 ||
      field > 255 ||
      bytes.length > 64 ||
      !Number.isInteger(revision) ||
      revision < 0 ||
      revision > 0xffffffff
    )
      return Promise.reject(new Error('Invalid module settings request.'));
    const next = turn.then(() => exchange(verb, field, revision, bytes));
    turn = next.then(
      () => {},
      () => {},
    );
    return next;
  }
  return {
    describe: () => request(0),
    get: (field) => submit(1, field),
    put: (field, value, revision) => submit(2, field, revision, value),
    reset: (field, revision) => submit(3, field, revision),
  };
}
