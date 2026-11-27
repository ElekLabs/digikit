// The shared vectors of the query codec: protobuf-es makes the bytes, the generated C codec must decode and re-encode them
// (packages/patchkit/tests/wire_roundtrip.rs).
import { create, fromBinary, toBinary, toJson, type DescMessage, type MessageInitShape } from '@bufbuild/protobuf';
import { CoverageSchema, ListReplySchema, ListRequestSchema, ProjectSchema, Mode } from './gen/project_manager_pb.js';

export type Case = {
  name: string;
  /** Message type name as the harness knows it. */
  message: string;
  input_hex: string;
  /** `ok`: decoded and encoded again to `output_hex`; `refuse`: decode must fail. */
  expect: 'ok' | 'refuse';
  output_hex?: string;
  /** The message as protobuf-es JSON (ok cases). */
  json?: unknown;
};

const schemas: Record<string, DescMessage> = {
  ListRequest: ListRequestSchema,
  Project: ProjectSchema,
  ListReply: ListReplySchema,
  Coverage: CoverageSchema,
};

const hex = (b: Uint8Array) => Buffer.from(b).toString('hex');

function ok(name: string, message: string, init: object, input?: string): Case {
  const schema = schemas[message] as DescMessage;
  const msg = create(schema, init as MessageInitShape<DescMessage>);
  const out = hex(toBinary(schema, msg));
  return { name, message, input_hex: input ?? out, expect: 'ok', output_hex: out, json: toJson(schema, msg) };
}

/** The expected message is what protobuf-es itself decodes from `input` (a case where the input is not canonical). */
function decoded(name: string, message: string, input: string): Case {
  const schema = schemas[message] as DescMessage;
  const msg = fromBinary(schema, Uint8Array.from(Buffer.from(input, 'hex')));
  const out = hex(toBinary(schema, msg));
  return { name, message, input_hex: input, expect: 'ok', output_hex: out, json: toJson(schema, msg) };
}

const refuse = (name: string, message: string, input: string): Case => ({
  name,
  message,
  input_hex: input,
  expect: 'refuse',
});

/** A seeded generator (mulberry32): the same numbers on every run. */
function rng(seed: number) {
  let a = seed;
  return (n: number) => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return Math.floor((((t ^ (t >>> 14)) >>> 0) / 4294967296) * n);
  };
}

const alphabet = ['a', 'Z', '0', ' ', '-', 'é', 'ü', '€', '音'];

/** A string of at most `max` UTF-8 bytes. */
function text(rand: (n: number) => number, max: number) {
  let s = '';
  for (let n = rand(max + 1); n > 0; n--) {
    const c = alphabet[rand(alphabet.length)] as string;
    if (Buffer.byteLength(s + c) <= max) s += c;
  }
  return s;
}

const pick = <T>(rand: (n: number) => number, xs: T[]) => xs[rand(xs.length)] as T;
const u32 = (rand: (n: number) => number) =>
  pick(rand, [0, 1, 127, 128, 300, 16384, 2 ** 21, 2 ** 28, 2 ** 32 - 1, rand(2 ** 32)]);
const i32 = (rand: (n: number) => number) =>
  pick(rand, [0, 1, -1, 127, -128, 2 ** 31 - 1, -(2 ** 31), rand(2 ** 32) - 2 ** 31]);

function project(rand: (n: number) => number) {
  return { slot: u32(rand), name: text(rand, 16), current: rand(2) === 1 };
}

function randomCases(count: number): Case[] {
  const rand = rng(20261128);
  const out: Case[] = [];
  for (let i = 0; i < count; i++) {
    const kind = i % 4;
    if (kind === 0) out.push(ok(`random-request-${i}`, 'ListRequest', { cursor: u32(rand) }));
    if (kind === 1) out.push(ok(`random-project-${i}`, 'Project', project(rand)));
    if (kind === 2) {
      const items = Array.from({ length: rand(17) }, () => project(rand));
      out.push(ok(`random-reply-${i}`, 'ListReply', { items, next: u32(rand) }));
    }
    if (kind === 3) {
      out.push(
        ok(`random-coverage-${i}`, 'Coverage', {
          mode: pick(rand, [Mode.OFF, Mode.SOFT, Mode.HARD, 7]),
          offset: i32(rand),
          blob: Uint8Array.from({ length: rand(9) }, () => rand(256)),
          levels: Array.from({ length: rand(7) }, () => u32(rand)),
          project: rand(2) ? project(rand) : undefined,
          flag: rand(2) === 1,
          count: u32(rand),
          label: text(rand, 5),
          tags: Array.from({ length: rand(4) }, () => text(rand, 4)),
          deltas: Array.from({ length: rand(5) }, () => i32(rand)),
          bits: Array.from({ length: rand(6) }, () => rand(2) === 1),
          modes: Array.from({ length: rand(4) }, () => pick(rand, [Mode.OFF, Mode.SOFT, Mode.HARD, 9])),
        }),
      );
    }
  }
  return out;
}

const full = Array.from({ length: 16 }, (_, slot) => ({
  slot,
  name: `project ${slot}`.padEnd(16, 'x'),
  current: slot === 3,
}));

export function cases(): Case[] {
  return [
    ok('request-empty', 'ListRequest', {}),
    ok('request-cursor', 'ListRequest', { cursor: 300 }),
    ok('request-max', 'ListRequest', { cursor: 0xffffffff }),
    ok('request-overlong-varint', 'ListRequest', { cursor: 1 }, '088100'),
    ok('request-last-duplicate-wins', 'ListRequest', { cursor: 9 }, '08010809'),
    ok(
      'request-skips-unknown',
      'ListRequest',
      { cursor: 5 },
      '7801' + '08' + '05' + '7203616263' + '6d01020304' + '61' + '0102030405060708',
    ),
    ok('project-typical', 'Project', { slot: 3, name: 'Demo', current: true }),
    ok('project-name-16', 'Project', { slot: 1, name: '0123456789abcdef' }),
    ok('project-all-default', 'Project', {}),
    ok('reply-empty', 'ListReply', {}),
    ok('reply-one', 'ListReply', { items: [{ slot: 2, name: 'x' }], next: 3 }),
    ok('reply-full', 'ListReply', { items: full, next: 16 }),
    ok('reply-empty-item', 'ListReply', { items: [{}] }),
    ok('coverage-all', 'Coverage', {
      mode: Mode.HARD,
      offset: -5,
      blob: Uint8Array.of(0, 1, 2, 255),
      levels: [1, 300, 70000],
      project: { slot: 7, name: 'p', current: true },
      flag: true,
      count: 9,
      label: 'abcde',
      tags: ['', 'ab', 'cdef'],
    }),
    ok('coverage-int32-min', 'Coverage', { offset: -(2 ** 31) }),
    ok('coverage-int32-max', 'Coverage', { offset: 2 ** 31 - 1 }),
    ok('coverage-unknown-enum-value', 'Coverage', { mode: 7 }),
    ok('coverage-empty-project-present', 'Coverage', { project: {} }),
    ok('coverage-blob-8', 'Coverage', { blob: new Uint8Array(8).fill(255) }),
    ok('coverage-levels-6', 'Coverage', { levels: [0, 1, 2, 3, 4, 5] }),
    ok('coverage-unpacked-levels', 'Coverage', { levels: [1, 2, 3] }, '200120022003'),
    ok('coverage-skips-unknown-in-nested', 'Coverage', { project: { slot: 1 } }, '2a06' + '0801' + '7801' + '7200'),
    ok('coverage-deltas', 'Coverage', { deltas: [0, -1, 2 ** 31 - 1, -(2 ** 31)] }),
    ok('coverage-deltas-4', 'Coverage', { deltas: [-1, -1, -1, -1] }),
    ok('coverage-bits', 'Coverage', { bits: [true, false, true, true, false] }),
    ok('coverage-modes', 'Coverage', { modes: [Mode.HARD, Mode.OFF, Mode.SOFT] }),
    ok('coverage-modes-unknown-value', 'Coverage', { modes: [1, 9, 300] }),
    ok('coverage-unpacked-deltas', 'Coverage', { deltas: [1, -2] }, '5001' + '50feffffffffffffffff01'),
    ok('coverage-unpacked-bits', 'Coverage', { bits: [true, false, true] }, '580158005801'),
    ok('coverage-unpacked-modes', 'Coverage', { modes: [Mode.SOFT, Mode.HARD] }, '60016002'),
    decoded('request-varint-bits-above-32', 'ListRequest', '08' + 'ffffffff7f'),
    decoded('request-varint-bit-32-only', 'ListRequest', '08' + '8080808010'),
    decoded('coverage-int32-from-5-byte-varint', 'Coverage', '10' + 'ffffffff0f'),
    decoded('coverage-int32-bits-above-32', 'Coverage', '10' + 'ffffffff7f'),
    ...randomCases(200),
    refuse('request-truncated-varint', 'ListRequest', '0880'),
    refuse('request-wrong-wire-type', 'ListRequest', '0a0100'),
    refuse('request-field-zero', 'ListRequest', '0001'),
    refuse('request-group', 'ListRequest', '0801' + '7b'),
    refuse('request-varint-11-bytes', 'ListRequest', '08' + '80'.repeat(10) + '00'),
    refuse('request-unknown-length-past-end', 'ListRequest', '720561'),
    refuse('request-unknown-fixed32-truncated', 'ListRequest', '6d010203'),
    refuse('project-name-17', 'Project', '1211' + '61'.repeat(17)),
    refuse('project-name-truncated', 'Project', '120461'),
    refuse('reply-17-items', 'ListReply', '0a00'.repeat(17)),
    refuse('reply-truncated-item', 'ListReply', '0a05' + '0801'),
    refuse('reply-item-truncated-name', 'ListReply', '0a03120561'),
    refuse('coverage-label-6', 'Coverage', '4206' + '61'.repeat(6)),
    refuse('coverage-blob-9', 'Coverage', '1a09' + '00'.repeat(9)),
    refuse('coverage-levels-7-packed', 'Coverage', '2207' + '01'.repeat(7)),
    refuse('coverage-levels-7-unpacked', 'Coverage', '2001'.repeat(7)),
    refuse('coverage-levels-packed-truncated', 'Coverage', '22' + '02' + '01' + '80'),
    refuse('coverage-deltas-5', 'Coverage', '5205' + '01'.repeat(5)),
    refuse('coverage-bits-6-unpacked', 'Coverage', '5801'.repeat(6)),
    refuse('coverage-modes-4-packed', 'Coverage', '6204' + '01'.repeat(4)),
    refuse('coverage-tag-5', 'Coverage', '4a05' + '61'.repeat(5)),
    refuse('coverage-4-tags', 'Coverage', '4a0161'.repeat(4)),
    // protobuf merges a repeated singular message; the device refuses it (the decoder has no room to merge)
    refuse('coverage-project-twice', 'Coverage', '2a00' + '2a00'),
  ];
}
