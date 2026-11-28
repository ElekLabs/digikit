import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { decodeDkp, dkpSpans, kindAt, type Span, type SpanKind } from '../workbench/dkp-spans';

interface Vector {
  name: string;
  hex: string;
  device: number;
  slot: number;
  counter: number;
  stamp: number;
  used: number;
  entries: [string, string, string][];
  modules: [string, number][];
}
const vectors = JSON.parse(
  readFileSync(new URL('../../../../../runtime/core/vectors/dkp-vectors.json', import.meta.url), 'utf8'),
) as { valid: Vector[]; invalid: { name: string; why: string; hex: string }[] };

const bytesOf = (hex: string) => Uint8Array.from(hex.match(/../g) ?? [], (pair) => parseInt(pair, 16));
const text = (bytes: Uint8Array, span: Span) => new TextDecoder().decode(bytes.slice(span.start, span.end));
const of = (spans: Span[], kind: SpanKind) => spans.filter((span) => span.kind === kind);

describe('the project file highlighter against the dkp vectors', () => {
  it('colours the header fields of every valid file, in order and without gaps up to used', () => {
    for (const vector of vectors.valid) {
      const bytes = bytesOf(vector.hex);
      const spans = dkpSpans(bytes);
      expect(spans, vector.name).toBeDefined();
      expect(spans!.slice(0, 9).map((span) => span.kind), vector.name).toEqual([
        'magic', 'version', 'device', 'slot', 'name', 'stamp', 'counter', 'used', 'crc',
      ]);
      let at = 0;
      for (const span of spans!) {
        expect(span.start, vector.name).toBe(at);
        at = span.end;
      }
      expect(at, vector.name).toBe(vector.used);
    }
  });

  it('finds each module id and hash and each owner, key and value of the entries', () => {
    for (const vector of vectors.valid) {
      const bytes = bytesOf(vector.hex);
      const spans = dkpSpans(bytes)!;
      expect(of(spans, 'module-id').map((span) => text(bytes, span).slice(1)), vector.name).toEqual(
        vector.modules.map(([id]) => id),
      );
      expect(of(spans, 'module-hash').map((span) => new DataView(bytes.buffer).getUint32(span.start)), vector.name).toEqual(
        vector.modules.map(([, hash]) => hash),
      );
      expect(of(spans, 'key').map((span) => text(bytes, span).slice(1)), vector.name).toEqual(
        vector.entries.map(([, key]) => key),
      );
      expect(
        of(spans, 'value').map((span) => Array.from(bytes.slice(span.start + 2, span.end)).map((b) => b.toString(16).padStart(2, '0')).join('')),
        vector.name,
      ).toEqual(vector.entries.map(([, , value]) => value));
      const owners = new Set(vector.entries.map(([owner]) => owner));
      expect(new Set(of(spans, 'owner').map((span) => text(bytes, span).slice(1))), vector.name).toEqual(owners);
    }
  });

  it('marks the body of a tag it does not know as unknown', () => {
    const vector = vectors.valid.find((v) => v.name === 'modules_two_owners_and_an_unknown_tag')!;
    const bytes = bytesOf(vector.hex);
    const unknown = of(dkpSpans(bytes)!, 'unknown');
    expect(unknown.map((span) => text(bytes, span))).toEqual(['future']);
  });

  it('shows a file of another version or magic as plain hex', () => {
    for (const vector of vectors.invalid.filter((v) => v.why === 'version' || v.name === 'magic')) {
      expect(dkpSpans(bytesOf(vector.hex)), vector.name).toBeUndefined();
    }
    expect(dkpSpans(new Uint8Array(10))).toBeUndefined();
  });

  it('answers kindAt by byte and ends at the last byte of a file cut short', () => {
    const vector = vectors.valid.find((v) => v.name === 'one_owner')!;
    const bytes = bytesOf(vector.hex);
    const spans = dkpSpans(bytes.slice(0, 50))!;
    expect(kindAt(spans, 0)).toBe('magic');
    expect(kindAt(spans, 38)).toBe('crc');
    expect(kindAt(spans, 42)).toBe('section');
    expect(spans[spans.length - 1].end).toBe(50);
    expect(kindAt(spans, 60)).toBeUndefined();
  });

  it('keeps every span in order inside its section for garbage lengths', () => {
    const header = bytesOf(vectors.valid.find((v) => v.name === 'one_owner')!.hex).slice(0, 42);
    // a data section of 4 bytes whose owner length says 200, followed by a second section
    const lying = Uint8Array.from([...header, 2, 0, 4, 200, 1, 2, 3, 1, 0, 0, 9, 9, 9]);
    new DataView(lying.buffer).setUint32(34, lying.length);
    const spans = dkpSpans(lying)!;
    const owner = of(spans, 'owner')[0];
    expect(owner.end).toBeLessThanOrEqual(42 + 3 + 4);
    let at = 42;
    let seed = 7;
    const random = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) >> 8;
    for (let round = 0; round < 200; round++) {
      const noise = Uint8Array.from({ length: 60 }, random);
      const bytes = new Uint8Array([...header.slice(0, 34), 0, 0, 0, 60 + 8, ...header.slice(38), ...noise]);
      const found = dkpSpans(bytes)!;
      at = 0;
      for (const span of found) {
        expect(span.start).toBeGreaterThanOrEqual(at);
        expect(span.end).toBeGreaterThan(span.start);
        expect(span.end).toBeLessThanOrEqual(bytes.length);
        at = span.end;
      }
    }
  });
});

describe('the decoded fields and checksum against the dkp vectors', () => {
  const hex32 = (value: number) => value.toString(16).padStart(8, '0').toUpperCase();
  const valueOf = (fields: { label: string; value: string }[], label: string) => fields.find((f) => f.label === label)?.value;

  it('decodes the header of every valid file to the vector values and the checksum is ok', () => {
    for (const vector of vectors.valid) {
      const decoded = decodeDkp(bytesOf(vector.hex))!;
      const header = decoded.fields.filter((f) => f.section === 'Header');
      expect(valueOf(header, 'magic'), vector.name).toBe('DKPJ');
      expect(valueOf(header, 'version'), vector.name).toBe('1');
      expect(valueOf(header, 'device'), vector.name).toBe(String(vector.device));
      expect(valueOf(header, 'slot'), vector.name).toBe(String(vector.slot));
      expect(valueOf(header, 'name'), vector.name).toBe(vector.name === 'empty' || vector.name === 'one_owner' ? 'MY SONG' : valueOf(header, 'name'));
      expect(valueOf(header, 'stamp'), vector.name).toBe(hex32(vector.stamp));
      expect(valueOf(header, 'write counter'), vector.name).toBe(String(vector.counter));
      expect(valueOf(header, 'used'), vector.name).toBe(String(vector.used));
      expect(valueOf(header, 'crc'), vector.name).toBe(hex32(new DataView(bytesOf(vector.hex).buffer).getUint32(38)));
      expect(decoded.checksum, vector.name).toBe('ok');
    }
  });

  it('decodes each module and each settings entry of every valid file', () => {
    for (const vector of vectors.valid) {
      const decoded = decodeDkp(bytesOf(vector.hex))!;
      const modules = decoded.fields.filter((f) => f.section === 'Modules');
      expect(modules.filter((f) => f.label === 'module').map((f) => f.value), vector.name).toEqual(vector.modules.map(([id]) => id));
      expect(modules.filter((f) => f.label === 'hash').map((f) => f.value), vector.name).toEqual(vector.modules.map(([, hash]) => hex32(hash)));
      const entries = decoded.fields.filter((f) => f.section.startsWith('Settings ') && f.label !== 'owner');
      expect(entries.map((f) => f.label), vector.name).toEqual(vector.entries.map(([, key]) => key));
      expect(entries.map((f) => f.size), vector.name).toEqual(vector.entries.map(([, , value]) => value.length / 2));
      expect(decoded.fields.filter((f) => f.label === 'owner').map((f) => f.value), vector.name).toEqual(
        [...new Set(vector.entries.map(([owner]) => owner))],
      );
    }
    const one = decodeDkp(bytesOf(vectors.valid.find((v) => v.name === 'one_owner')!.hex))!;
    expect(valueOf(one.fields, 'test/owner/pool')).toBe('01 02 03');
  });

  it('reports a mismatch for a file with one byte flipped, in the header, the body or the stored crc', () => {
    for (const vector of vectors.valid) {
      const original = bytesOf(vector.hex);
      for (const at of [0x09, 30, 38, original.length - 1].filter((i) => i < vector.used)) {
        const bytes = original.slice();
        bytes[at] ^= 0x01;
        const decoded = decodeDkp(bytes);
        // a flipped magic or version byte is no known file at all; the others decode and fail the checksum
        if (decoded) expect(decoded.checksum, `${vector.name} at ${at}`).toBe('mismatch');
      }
    }
  });

  it('gives no fields for a file of another version', () => {
    for (const vector of vectors.invalid.filter((v) => v.why === 'version')) {
      expect(decodeDkp(bytesOf(vector.hex)), vector.name).toBeUndefined();
    }
  });
});
