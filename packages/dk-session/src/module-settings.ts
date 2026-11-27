// A public settings endpoint belongs to one loaded module instance on one session.
import { CMD } from './gen_proto.ts';
import { be32, deviceError, statusName, u32, type DkSession } from './dkupload-core.ts';
import { poll } from './waits.ts';

export type PublicField = {
  id: string;
  label: string;
  maxBytes: number;
  apply: 'immediate' | 'next-restart';
} & ({ kind: 'text' } | { kind: 'file-name'; extension: string; resetLabel: string });

export interface PublicModuleSettings {
  name: string;
  settings: PublicField[];
}

export interface PublicSettingValue {
  value?: string;
  revision: number;
}

export interface ModuleSettingsEndpoint {
  describe(): Promise<PublicModuleSettings>;
  get(field: number): Promise<PublicSettingValue>;
  put(field: number, value: string, revision: number): Promise<PublicSettingValue>;
  reset(field: number, revision: number): Promise<PublicSettingValue>;
}

const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });

export function decodePublicSettings(bytes: Uint8Array): PublicModuleSettings {
  let at = 0;
  function byte() {
    if (at >= bytes.length) throw new Error('Incomplete module descriptor.');
    return bytes[at++]!;
  }
  function text() {
    const len = byte();
    if (!len || at + len > bytes.length) throw new Error('Invalid module descriptor string.');
    const value = decoder.decode(bytes.subarray(at, at + len));
    at += len;
    return value;
  }
  if (bytes.length > 512 || byte() !== 1) throw new Error('Unsupported module descriptor.');
  const name = text();
  const settings: PublicField[] = [];
  const count = byte();
  for (let i = 0; i < count; i++) {
    const id = text();
    const label = text();
    const maxBytes = byte();
    const kind = byte();
    const timing = byte();
    if (!maxBytes || maxBytes > 64 || settings.some((f) => f.id === id)) throw new Error('Invalid public setting.');
    if (timing > 1 || kind > 1) throw new Error('Unsupported module setting.');
    const apply = timing ? 'next-restart' : 'immediate';
    if (!kind) {
      settings.push({ id, label, maxBytes, apply, kind: 'text' });
    } else {
      const extension = text();
      if (maxBytes > 47 || !/^[a-z0-9]{1,16}$/.test(extension)) throw new Error('Invalid file setting.');
      settings.push({ id, label, maxBytes, apply, kind: 'file-name', extension, resetLabel: text() });
    }
  }
  if (at !== bytes.length) throw new Error('Unexpected bytes in module descriptor.');
  return { name, settings };
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
  async function exchange(verb: number, field: number, revision: number, value: Uint8Array) {
    const ticket = be32(crypto.getRandomValues(new Uint32Array(1))[0]! || 1);
    await request(verb, Uint8Array.of(...ticket, field, ...be32(revision), ...value));
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
        throw new Error('The setting changed. Read it again before saving.');
      throw deviceError('Module settings', result);
    }
    if (bytes[9]! > 1 || (!bytes[9] && bytes.length !== 10)) throw new Error('Invalid module setting value.');
    return { revision: u32(bytes, 5), value: bytes[9] ? decoder.decode(bytes.subarray(10)) : undefined };
  }
  function submit(verb: number, field: number, revision = 0, value = '') {
    const bytes = encoder.encode(value);
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
    describe: async () => decodePublicSettings(await request(0)),
    get: (field) => submit(1, field),
    put: (field, value, revision) => submit(2, field, revision, value),
    reset: (field, revision) => submit(3, field, revision),
  };
}
