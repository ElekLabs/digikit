// The one MODULE_UI request of a loaded module instance: `[verb][slot][instance u32][suffix]`. Settings and queries share it.
import { CMD } from './gen_proto.ts';
import { be32, deviceError, type DkSession } from './dkupload-core.ts';

export function moduleUiRequester(
  session: Pick<DkSession, 'request'>,
  slot: number,
  instance: number,
  current: () => boolean,
) {
  const check = () => {
    if (!current()) throw new Error('The module page has closed.');
  };
  return async (verb: number, suffix: Uint8Array = new Uint8Array(), what = 'Module settings') => {
    check();
    const body = Uint8Array.of(verb, slot, ...be32(instance), ...suffix);
    const reply = await session.request(CMD.MODULE_UI, body);
    check();
    if (reply.status) throw deviceError(what, reply.status);
    return reply.body;
  };
}
