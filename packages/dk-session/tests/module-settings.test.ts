import { expect, test } from 'vitest';
import { be32, wireModuleSettings, type DkSession } from '../src/index.ts';
import { CMD } from '../src/gen_proto.ts';

const utf8 = (s: string) => new TextEncoder().encode(s);
const string = (s: string) => [utf8(s).length, ...utf8(s)];
const schema = Uint8Array.of(
  1,
  ...string('Animations'),
  2,
  ...string('active'),
  ...string('Animation'),
  47,
  1,
  1,
  ...string('dkbs'),
  ...string('Stock'),
  ...string('caption'),
  ...string('Caption'),
  24,
  0,
  0,
);
const done = (revision: number, value?: string, code = 0) =>
  Uint8Array.of(0, ...be32(code), ...be32(revision), Number(value !== undefined), ...utf8(value ?? ''));

function scripted(replies: Uint8Array[]) {
  const requests: Uint8Array[] = [];
  let now = 0;
  const session: Pick<DkSession, 'clock' | 'request'> = {
    clock: {
      now: async () => now,
      pause: async (ms) => {
        now += ms;
      },
    },
    async request(cmd, body = new Uint8Array()) {
      expect(cmd).toBe(CMD.MODULE_UI);
      requests.push(body);
      const reply = replies.shift();
      if (!reply) throw new Error('Unexpected request.');
      return { cmd, seq: 0, status: 0, body: reply };
    },
  };
  return { session, requests };
}

test('the same endpoint describes file choices and text and saves and resets a file choice', async () => {
  const accepted = new Uint8Array();
  const { session, requests } = scripted([
    schema,
    accepted,
    Uint8Array.of(1),
    done(1),
    accepted,
    done(2, 'intro.dkbs'),
    accepted,
    done(2, 'intro.dkbs', -88),
    accepted,
    done(3),
  ]);
  const endpoint = wireModuleSettings(session, 3, 0x1234, () => true);
  expect(await endpoint.describe()).toEqual({
    name: 'Animations',
    settings: [
      {
        id: 'active',
        label: 'Animation',
        maxBytes: 47,
        kind: 'file-name',
        extension: 'dkbs',
        resetLabel: 'Stock',
        apply: 'next-restart',
      },
      { id: 'caption', label: 'Caption', maxBytes: 24, kind: 'text', apply: 'immediate' },
    ],
  });
  const old = await endpoint.get(0);
  expect(old.value).toBeUndefined();
  const selected = await endpoint.put(0, 'intro.dkbs', old.revision);
  expect(selected.value).toBe('intro.dkbs');
  await expect(endpoint.reset(0, old.revision)).rejects.toThrow(/changed/);
  expect((await endpoint.reset(0, selected.revision)).value).toBeUndefined();
  const submit = requests.find((request) => request[0] === 2)!;
  expect(Array.from(submit.slice(0, 6))).toEqual([2, 3, 0, 0, 0x12, 0x34]);
  expect(Array.from(submit.slice(10))).toEqual([0, 0, 0, 0, 1, ...utf8('intro.dkbs')]);
  const poll = requests[requests.indexOf(submit) + 1]!;
  expect(poll.slice(6)).toEqual(submit.slice(6, 10));
});

test('a delayed result cannot resolve after the page or target closes', async () => {
  const { session } = scripted([]);
  let active = true;
  session.request = async (cmd) => {
    active = false;
    return { cmd, status: 0, seq: 0, body: schema };
  };
  await expect(wireModuleSettings(session, 0, 7, () => active).describe()).rejects.toThrow(/closed/);
});
